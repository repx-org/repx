use crate::error::{ClientError, Result};
use repx_core::errors::CoreError;
use repx_core::fs_utils::path_to_string;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use tar::EntryType;

fn io_err(e: std::io::Error) -> ClientError {
    ClientError::Io(e)
}

fn io_err_ctx(e: std::io::Error, msg: impl std::fmt::Display) -> ClientError {
    ClientError::Io(std::io::Error::new(e.kind(), format!("{}: {}", msg, e)))
}

fn strip_tar_prefix(raw_str: &str) -> Option<&str> {
    let idx = raw_str.find('/')?;
    let rest = &raw_str[idx + 1..];
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

fn write_regular_entry<R: Read>(entry: &mut tar::Entry<'_, R>, dest_path: &Path) -> Result<()> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err)?;
    }
    let _ = std::fs::remove_file(dest_path);
    let mut out_file = std::fs::File::create(dest_path).map_err(io_err)?;
    std::io::copy(entry, &mut out_file).map_err(io_err)?;
    if let Ok(mode) = entry.header().mode() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest_path, std::fs::Permissions::from_mode(mode));
    }
    Ok(())
}

fn extract_hardlink_target<R: Read>(
    entry: &tar::Entry<'_, R>,
    stripped: &str,
) -> Result<(String, String)> {
    let link_target = entry
        .link_name()
        .map_err(|e| io_err_ctx(e, "Invalid hardlink target"))?
        .ok_or_else(|| {
            ClientError::Config(CoreError::InvalidConfig {
                detail: format!("Hardlink '{}' has no target", stripped),
            })
        })?;
    let target_str = path_to_string(&*link_target);
    let stripped_target = match target_str.find('/') {
        Some(idx) => target_str[idx + 1..].to_string(),
        None => target_str,
    };
    Ok((stripped.to_string(), stripped_target))
}

fn resolve_hardlinks(
    hardlinks: &[(String, String)],
    dest_dir: &Path,
) -> Result<Vec<(String, String)>> {
    let link_map: HashMap<&str, &str> = hardlinks
        .iter()
        .map(|(l, t)| (l.as_str(), t.as_str()))
        .collect();

    let mut unresolved = Vec::new();

    for (link_path, _) in hardlinks {
        let ultimate_target =
            repx_core::fs_utils::resolve_link_chain_ref(&link_map, link_path.as_str(), 100);
        let src = dest_dir.join(ultimate_target);
        let dst = dest_dir.join(link_path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
        let _ = std::fs::remove_file(&dst);
        if src.exists() {
            std::fs::copy(&src, &dst).map_err(|e| {
                io_err_ctx(
                    e,
                    format!(
                        "Failed to copy hardlink '{}' -> '{}'",
                        link_path, ultimate_target
                    ),
                )
            })?;
            if let Ok(meta) = std::fs::metadata(&src) {
                let _ = std::fs::set_permissions(&dst, meta.permissions());
            }
        } else {
            unresolved.push((link_path.clone(), ultimate_target.to_string()));
        }
    }

    Ok(unresolved)
}

fn write_data_to_links(
    dest_dir: &Path,
    link_paths: &[String],
    data: &[u8],
    mode: Option<u32>,
) -> Result<()> {
    for link_path in link_paths {
        let dst = dest_dir.join(link_path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
        let _ = std::fs::remove_file(&dst);
        std::fs::write(&dst, data).map_err(io_err)?;
        if let Some(m) = mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(m));
        }
    }
    Ok(())
}

fn resolve_unresolved_from_tar(
    tar_path: &Path,
    dest_dir: &Path,
    unresolved: &[(String, String)],
    resolve_fs_symlinks: bool,
) -> Result<()> {
    if unresolved.is_empty() {
        return Ok(());
    }

    let mut needed: HashMap<String, Vec<String>> = HashMap::new();
    for (link_path, target_path) in unresolved {
        needed
            .entry(target_path.clone())
            .or_default()
            .push(link_path.clone());
    }

    let file = std::fs::File::open(tar_path).map_err(io_err)?;
    let mut archive = tar::Archive::new(file);

    for entry_result in archive
        .entries()
        .map_err(|e| io_err_ctx(e, "Failed to read tar entries (pass 2)"))?
    {
        if needed.is_empty() {
            break;
        }

        let mut entry =
            entry_result.map_err(|e| io_err_ctx(e, "Failed to read tar entry (pass 2)"))?;

        let raw_path = entry
            .path()
            .map_err(|e| io_err_ctx(e, "Invalid path in tar"))?
            .to_path_buf();
        let raw_str = path_to_string(&raw_path);
        let stripped = match strip_tar_prefix(&raw_str) {
            Some(s) => s,
            None => continue,
        };

        if let Some(link_paths) = needed.remove(stripped) {
            let entry_type = entry.header().entry_type();

            let data_and_mode = if entry_type.is_file() {
                let mut data = Vec::new();
                entry.read_to_end(&mut data).map_err(io_err)?;
                let mode = entry.header().mode().ok();
                Some((data, mode))
            } else if resolve_fs_symlinks && entry_type.is_symlink() {
                resolve_symlink_from_fs(&entry)
            } else {
                None
            };

            if let Some((data, mode)) = data_and_mode {
                write_data_to_links(dest_dir, &link_paths, &data, mode)?;
            } else {
                needed.insert(stripped.to_string(), link_paths);
            }
        }
    }

    Ok(())
}

fn resolve_symlink_from_fs<R: Read>(entry: &tar::Entry<'_, R>) -> Option<(Vec<u8>, Option<u32>)> {
    let target = entry.link_name().ok()??.to_path_buf();
    if !target.is_absolute() || !target.exists() {
        return None;
    }
    let data = std::fs::read(&target).ok()?;
    let mode = std::fs::metadata(&target).ok().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode()
    });
    Some((data, mode))
}

fn extract_filtered(
    tar_path: &Path,
    dest_dir: &Path,
    prefix: Option<&str>,
) -> Result<(bool, Vec<(String, String)>)> {
    let file = std::fs::File::open(tar_path).map_err(io_err)?;
    let mut archive = tar::Archive::new(file);
    let mut hardlinks: Vec<(String, String)> = Vec::new();
    let mut found = false;
    let needle = prefix.map(|p| p.trim_end_matches('/'));

    for entry_result in archive
        .entries()
        .map_err(|e| io_err_ctx(e, "Failed to read tar entries"))?
    {
        let mut entry = entry_result.map_err(|e| io_err_ctx(e, "Failed to read tar entry"))?;
        let raw_path = entry
            .path()
            .map_err(|e| io_err_ctx(e, "Invalid path in tar"))?
            .to_path_buf();
        let raw_str = path_to_string(&raw_path);
        let stripped = match strip_tar_prefix(&raw_str) {
            Some(s) => s,
            None => continue,
        };

        if let Some(needle) = needle {
            if !stripped.starts_with(needle) {
                continue;
            }
            let after = &stripped[needle.len()..];
            if !after.is_empty() && !after.starts_with('/') {
                continue;
            }
        }

        found = true;
        let dest_path = dest_dir.join(stripped);

        match entry.header().entry_type() {
            EntryType::Directory => {
                std::fs::create_dir_all(&dest_path).map_err(io_err)?;
            }
            EntryType::Regular | EntryType::GNUSparse => {
                write_regular_entry(&mut entry, &dest_path)?;
            }
            EntryType::Symlink => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent).map_err(io_err)?;
                }
                if let Ok(Some(target)) = entry.link_name().map(|o| o.map(|p| p.to_path_buf())) {
                    let _ = std::fs::remove_file(&dest_path);
                    std::os::unix::fs::symlink(&target, &dest_path).map_err(io_err)?;
                }
            }
            EntryType::Link => {
                hardlinks.push(extract_hardlink_target(&entry, stripped)?);
            }
            _ => {}
        }
    }

    let unresolved = if hardlinks.is_empty() {
        Vec::new()
    } else {
        resolve_hardlinks(&hardlinks, dest_dir)?
    };

    Ok((found, unresolved))
}

pub(crate) fn extract_tar_to_dir(tar_path: &Path, dest_dir: &Path) -> Result<()> {
    let (_found, unresolved) = extract_filtered(tar_path, dest_dir, None)?;
    for (link_path, target) in &unresolved {
        tracing::warn!(
            "Hardlink target '{}' not found for '{}', skipping",
            target,
            link_path
        );
    }
    Ok(())
}

pub(crate) fn extract_image_from_tar(
    tar_path: &Path,
    image_rel_path: &str,
    dest_dir: &Path,
) -> Result<()> {
    let (found, unresolved) = extract_filtered(tar_path, dest_dir, Some(image_rel_path))?;
    resolve_unresolved_from_tar(tar_path, dest_dir, &unresolved, false)?;
    if !found {
        tracing::warn!("Image '{}' not found in tar {:?}", image_rel_path, tar_path);
    }
    Ok(())
}

pub(crate) fn extract_host_tools_from_tar(tar_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(tar_path).map_err(io_err)?;
    let mut archive = tar::Archive::new(file);

    let mut found_host_tools = false;
    let mut hardlinks: Vec<(String, String)> = Vec::new();

    for entry_result in archive
        .entries()
        .map_err(|e| io_err_ctx(e, "Failed to read tar entries"))?
    {
        let mut entry = entry_result.map_err(|e| io_err_ctx(e, "Failed to read tar entry"))?;
        let raw_path = entry
            .path()
            .map_err(|e| io_err_ctx(e, "Invalid path in tar"))?
            .to_path_buf();
        let raw_str = path_to_string(&raw_path);
        let stripped = match strip_tar_prefix(&raw_str) {
            Some(s) => s,
            None => continue,
        };

        if !stripped.starts_with("host-tools/") && stripped != "host-tools" {
            if found_host_tools {
                break;
            }
            continue;
        }
        found_host_tools = true;

        let dest_path = dest_dir.join(stripped);

        match entry.header().entry_type() {
            EntryType::Directory => {
                std::fs::create_dir_all(&dest_path).map_err(io_err)?;
            }
            EntryType::Regular | EntryType::GNUSparse => {
                write_regular_entry(&mut entry, &dest_path)?;
            }
            EntryType::Symlink => {
                if let Ok(Some(target)) = entry.link_name().map(|o| o.map(|p| p.to_path_buf())) {
                    let parent_in_tar = std::path::Path::new(stripped)
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    let mut resolved = parent_in_tar.to_path_buf();
                    for component in target.components() {
                        match component {
                            std::path::Component::ParentDir => {
                                resolved.pop();
                            }
                            std::path::Component::Normal(c) => {
                                resolved.push(c);
                            }
                            std::path::Component::CurDir => {}
                            _ => {
                                resolved.push(component.as_os_str());
                            }
                        }
                    }
                    let resolved_str = path_to_string(&resolved);
                    if resolved_str.starts_with("host-tools/") {
                        if let Some(parent) = dest_path.parent() {
                            std::fs::create_dir_all(parent).map_err(io_err)?;
                        }
                        let _ = std::fs::remove_file(&dest_path);
                        std::os::unix::fs::symlink(&target, &dest_path).map_err(io_err)?;
                    } else {
                        hardlinks.push((stripped.to_string(), resolved_str));
                    }
                }
            }
            EntryType::Link => {
                hardlinks.push(extract_hardlink_target(&entry, stripped)?);
            }
            _ => {}
        }
    }

    if !hardlinks.is_empty() {
        let unresolved = resolve_hardlinks(&hardlinks, dest_dir)?;
        resolve_unresolved_from_tar(tar_path, dest_dir, &unresolved, true)?;
    }

    if !found_host_tools {
        return Err(ClientError::Config(CoreError::InvalidConfig {
            detail: format!("No host-tools/ directory found in tar {:?}", tar_path),
        }));
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_extract_host_tools_from_tar() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("test.tar");

        let file = File::create(&tar_path).unwrap();
        let mut builder = tar::Builder::new(file);

        let add_dir = |b: &mut tar::Builder<File>, p: &str| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, p, &[][..]).unwrap();
        };
        let add_file = |b: &mut tar::Builder<File>, p: &str, c: &[u8], mode: u32| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(c.len() as u64);
            h.set_mode(mode);
            h.set_cksum();
            b.append_data(&mut h, p, c).unwrap();
        };

        add_dir(&mut builder, "mylab-prefix");
        add_dir(&mut builder, "mylab-prefix/host-tools");
        add_dir(&mut builder, "mylab-prefix/host-tools/abc123");
        add_dir(&mut builder, "mylab-prefix/host-tools/abc123/bin");

        add_file(
            &mut builder,
            "mylab-prefix/host-tools/abc123/bin/[",
            b"COREUTILS_BINARY",
            0o755,
        );
        add_file(
            &mut builder,
            "mylab-prefix/host-tools/abc123/bin/rsync",
            b"RSYNC_BINARY",
            0o755,
        );
        {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Link);
            h.set_size(0);
            h.set_mode(0o755);
            h.set_cksum();
            builder
                .append_link(
                    &mut h,
                    "mylab-prefix/host-tools/abc123/bin/cat",
                    "mylab-prefix/host-tools/abc123/bin/[",
                )
                .unwrap();
        }

        add_dir(&mut builder, "mylab-prefix/images");
        add_file(
            &mut builder,
            "mylab-prefix/images/big.tar",
            b"HUGE_IMAGE_DATA",
            0o644,
        );

        builder.finish().unwrap();

        let dest = dir.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_host_tools_from_tar(&tar_path, &dest).unwrap();

        let rsync = dest.join("host-tools/abc123/bin/rsync");
        assert!(rsync.exists(), "rsync must exist");
        assert_eq!(std::fs::read(&rsync).unwrap(), b"RSYNC_BINARY");

        let bracket = dest.join("host-tools/abc123/bin/[");
        assert!(bracket.exists(), "[ must exist");
        assert_eq!(std::fs::read(&bracket).unwrap(), b"COREUTILS_BINARY");

        let cat = dest.join("host-tools/abc123/bin/cat");
        assert!(cat.exists(), "cat must exist");
        assert_eq!(std::fs::read(&cat).unwrap(), b"COREUTILS_BINARY");

        assert!(
            !dest.join("images").exists(),
            "images/ must NOT be extracted"
        );
    }
}
