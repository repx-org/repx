use crate::error::{ClientError, Result};
use repx_core::errors::CoreError;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use tar::EntryType;

pub(crate) fn extract_tar_to_dir(tar_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(tar_path).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
    let mut archive = tar::Archive::new(file);

    let mut hardlinks: Vec<(String, String)> = Vec::new();

    for entry_result in archive.entries().map_err(|e| {
        ClientError::Config(CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries: {}", e),
        )))
    })? {
        let mut entry = entry_result.map_err(|e| {
            ClientError::Config(CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read tar entry: {}", e),
            )))
        })?;

        let raw_path = entry
            .path()
            .map_err(|e| {
                ClientError::Config(CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Invalid path in tar: {}", e),
                )))
            })?
            .to_path_buf();
        let raw_str = raw_path.to_string_lossy().to_string();

        let stripped = match raw_str.find('/') {
            Some(idx) => &raw_str[idx + 1..],
            None => continue,
        };
        if stripped.is_empty() {
            continue;
        }

        let dest_path = dest_dir.join(stripped);
        let entry_type = entry.header().entry_type();

        match entry_type {
            EntryType::Directory => {
                std::fs::create_dir_all(&dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            }
            EntryType::Regular | EntryType::GNUSparse => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                }
                let _ = std::fs::remove_file(&dest_path);
                let mut out_file = std::fs::File::create(&dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                std::io::copy(&mut entry, &mut out_file)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;

                if let Ok(mode) = entry.header().mode() {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(mode));
                }
            }
            EntryType::Symlink => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                }
                let link_target = entry
                    .link_name()
                    .map_err(|e| {
                        ClientError::Config(CoreError::Io(std::io::Error::new(
                            e.kind(),
                            format!("Invalid symlink target in tar: {}", e),
                        )))
                    })?
                    .ok_or_else(|| {
                        ClientError::Config(CoreError::InvalidConfig {
                            detail: format!("Symlink entry '{}' has no target", stripped),
                        })
                    })?;
                let _ = std::fs::remove_file(&dest_path);
                std::os::unix::fs::symlink(link_target.as_ref(), &dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            }
            EntryType::Link => {
                let link_target = entry
                    .link_name()
                    .map_err(|e| {
                        ClientError::Config(CoreError::Io(std::io::Error::new(
                            e.kind(),
                            format!("Invalid hardlink target in tar: {}", e),
                        )))
                    })?
                    .ok_or_else(|| {
                        ClientError::Config(CoreError::InvalidConfig {
                            detail: format!("Hardlink entry '{}' has no target", stripped),
                        })
                    })?;
                let target_str = link_target.to_string_lossy().to_string();
                let stripped_target = match target_str.find('/') {
                    Some(idx) => target_str[idx + 1..].to_string(),
                    None => target_str,
                };
                hardlinks.push((stripped.to_string(), stripped_target));
            }
            _ => {}
        }
    }

    let link_map: HashMap<&str, &str> = hardlinks
        .iter()
        .map(|(link, target)| (link.as_str(), target.as_str()))
        .collect();

    for (link_path, _) in &hardlinks {
        let mut current = link_path.as_str();
        let mut depth = 0;
        while let Some(&next) = link_map.get(current) {
            current = next;
            depth += 1;
            if depth > 100 {
                break;
            }
        }
        let ultimate_target = current;
        let src = dest_dir.join(ultimate_target);
        let dst = dest_dir.join(link_path);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
        }
        let _ = std::fs::remove_file(&dst);

        if src.exists() {
            std::fs::copy(&src, &dst).map_err(|e| {
                ClientError::Config(CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to copy hardlink '{}' -> '{}': {}",
                        link_path, ultimate_target, e
                    ),
                )))
            })?;
            if let Ok(meta) = std::fs::metadata(&src) {
                let _ = std::fs::set_permissions(&dst, meta.permissions());
            }
        } else {
            tracing::warn!(
                "Hardlink target '{}' not found for '{}', skipping",
                ultimate_target,
                link_path
            );
        }
    }

    Ok(())
}

pub(crate) fn extract_image_from_tar(
    tar_path: &Path,
    image_rel_path: &str,
    dest_dir: &Path,
) -> Result<()> {
    let file = std::fs::File::open(tar_path).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
    let mut archive = tar::Archive::new(file);

    let needle = image_rel_path.trim_end_matches('/');
    let mut found = false;

    let mut hardlinks: Vec<(String, String)> = Vec::new();

    for entry_result in archive.entries().map_err(|e| {
        ClientError::Config(CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries: {}", e),
        )))
    })? {
        let mut entry = entry_result.map_err(|e| {
            ClientError::Config(CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read tar entry: {}", e),
            )))
        })?;

        let raw_path = entry
            .path()
            .map_err(|e| {
                ClientError::Config(CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Invalid path in tar: {}", e),
                )))
            })?
            .to_path_buf();
        let raw_str = raw_path.to_string_lossy().to_string();

        let stripped = match raw_str.find('/') {
            Some(idx) => &raw_str[idx + 1..],
            None => continue,
        };
        if stripped.is_empty() {
            continue;
        }

        if !stripped.starts_with(needle) {
            continue;
        }
        let after = &stripped[needle.len()..];
        if !after.is_empty() && !after.starts_with('/') {
            continue;
        }

        let dest_path = dest_dir.join(stripped);
        let entry_type = entry.header().entry_type();

        match entry_type {
            EntryType::Directory => {
                std::fs::create_dir_all(&dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                found = true;
            }
            EntryType::Regular | EntryType::GNUSparse => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                }
                let _ = std::fs::remove_file(&dest_path);
                let mut out_file = std::fs::File::create(&dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                std::io::copy(&mut entry, &mut out_file)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                if let Ok(mode) = entry.header().mode() {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(mode));
                }
                found = true;
            }
            EntryType::Link => {
                let link_target = entry
                    .link_name()
                    .map_err(|e| {
                        ClientError::Config(CoreError::Io(std::io::Error::new(
                            e.kind(),
                            format!("Invalid hardlink target: {}", e),
                        )))
                    })?
                    .ok_or_else(|| {
                        ClientError::Config(CoreError::InvalidConfig {
                            detail: format!("Hardlink '{}' has no target", stripped),
                        })
                    })?;
                let target_str = link_target.to_string_lossy().to_string();
                let stripped_target = match target_str.find('/') {
                    Some(idx) => target_str[idx + 1..].to_string(),
                    None => target_str,
                };
                hardlinks.push((stripped.to_string(), stripped_target));
                found = true;
            }
            EntryType::Symlink => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                }
                if let Ok(Some(target)) = entry.link_name().map(|o| o.map(|p| p.to_path_buf())) {
                    let _ = std::fs::remove_file(&dest_path);
                    std::os::unix::fs::symlink(&target, &dest_path)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                }
                found = true;
            }
            _ => {}
        }
    }

    if !hardlinks.is_empty() {
        let mut unresolved: Vec<(String, String)> = Vec::new();
        let link_map: HashMap<&str, &str> = hardlinks
            .iter()
            .map(|(l, t)| (l.as_str(), t.as_str()))
            .collect();

        for (link_path, _) in &hardlinks {
            let mut current = link_path.as_str();
            let mut depth = 0;
            while let Some(&next) = link_map.get(current) {
                current = next;
                depth += 1;
                if depth > 100 {
                    break;
                }
            }
            let src = dest_dir.join(current);
            let dst = dest_dir.join(link_path);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            }
            let _ = std::fs::remove_file(&dst);
            if src.exists() {
                std::fs::copy(&src, &dst).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                if let Ok(meta) = std::fs::metadata(&src) {
                    let _ = std::fs::set_permissions(&dst, meta.permissions());
                }
            } else {
                unresolved.push((link_path.clone(), current.to_string()));
            }
        }

        if !unresolved.is_empty() {
            let needed: HashMap<String, Vec<String>> = {
                let mut m: HashMap<String, Vec<String>> = HashMap::new();
                for (link_path, target_path) in &unresolved {
                    m.entry(target_path.clone())
                        .or_default()
                        .push(link_path.clone());
                }
                m
            };

            let file =
                std::fs::File::open(tar_path).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            let mut archive2 = tar::Archive::new(file);

            for entry_result in archive2.entries().map_err(|e| {
                ClientError::Config(CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read tar entries (pass 2): {}", e),
                )))
            })? {
                let mut entry = entry_result.map_err(|e| {
                    ClientError::Config(CoreError::Io(std::io::Error::new(
                        e.kind(),
                        format!("Failed to read tar entry (pass 2): {}", e),
                    )))
                })?;

                if !entry.header().entry_type().is_file() {
                    continue;
                }

                let raw_path = entry
                    .path()
                    .map_err(|e| {
                        ClientError::Config(CoreError::Io(std::io::Error::new(
                            e.kind(),
                            format!("Invalid path in tar: {}", e),
                        )))
                    })?
                    .to_path_buf();
                let raw_str = raw_path.to_string_lossy().to_string();
                let stripped = match raw_str.find('/') {
                    Some(idx) => &raw_str[idx + 1..],
                    None => continue,
                };

                if let Some(link_paths) = needed.get(stripped) {
                    let mut data = Vec::new();
                    entry
                        .read_to_end(&mut data)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                    let mode = entry.header().mode().ok();

                    for link_path in link_paths {
                        let dst = dest_dir.join(link_path);
                        if let Some(parent) = dst.parent() {
                            std::fs::create_dir_all(parent)
                                .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                        }
                        let _ = std::fs::remove_file(&dst);
                        std::fs::write(&dst, &data)
                            .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                        if let Some(m) = mode {
                            use std::os::unix::fs::PermissionsExt;
                            let _ =
                                std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(m));
                        }
                    }
                }
            }
        }
    }

    if !found {
        tracing::warn!("Image '{}' not found in tar {:?}", image_rel_path, tar_path);
    }

    Ok(())
}

pub(crate) fn extract_host_tools_from_tar(tar_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(tar_path).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
    let mut archive = tar::Archive::new(file);

    let mut found_host_tools = false;
    let mut hardlinks: Vec<(String, String)> = Vec::new();

    for entry_result in archive.entries().map_err(|e| {
        ClientError::Config(CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("Failed to read tar entries: {}", e),
        )))
    })? {
        let mut entry = entry_result.map_err(|e| {
            ClientError::Config(CoreError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to read tar entry: {}", e),
            )))
        })?;

        let raw_path = entry
            .path()
            .map_err(|e| {
                ClientError::Config(CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Invalid path in tar: {}", e),
                )))
            })?
            .to_path_buf();
        let raw_str = raw_path.to_string_lossy().to_string();

        let stripped = match raw_str.find('/') {
            Some(idx) => &raw_str[idx + 1..],
            None => continue,
        };
        if stripped.is_empty() {
            continue;
        }

        if !stripped.starts_with("host-tools/") && stripped != "host-tools" {
            if found_host_tools {
                break;
            }
            continue;
        }
        found_host_tools = true;

        let dest_path = dest_dir.join(stripped);
        let entry_type = entry.header().entry_type();

        match entry_type {
            EntryType::Directory => {
                std::fs::create_dir_all(&dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            }
            EntryType::Regular | EntryType::GNUSparse => {
                if let Some(parent) = dest_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                }
                let _ = std::fs::remove_file(&dest_path);
                let mut out_file = std::fs::File::create(&dest_path)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                std::io::copy(&mut entry, &mut out_file)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                if let Ok(mode) = entry.header().mode() {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(mode));
                }
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
                    let resolved_str = resolved.to_string_lossy().to_string();
                    if resolved_str.starts_with("host-tools/") {
                        if let Some(parent) = dest_path.parent() {
                            std::fs::create_dir_all(parent)
                                .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                        }
                        let _ = std::fs::remove_file(&dest_path);
                        std::os::unix::fs::symlink(&target, &dest_path)
                            .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                    } else {
                        hardlinks.push((stripped.to_string(), resolved_str));
                    }
                }
            }
            EntryType::Link => {
                let link_target = entry
                    .link_name()
                    .map_err(|e| {
                        ClientError::Config(CoreError::Io(std::io::Error::new(
                            e.kind(),
                            format!("Invalid hardlink target: {}", e),
                        )))
                    })?
                    .ok_or_else(|| {
                        ClientError::Config(CoreError::InvalidConfig {
                            detail: format!("Hardlink '{}' has no target", stripped),
                        })
                    })?;
                let target_str = link_target.to_string_lossy().to_string();
                let stripped_target = match target_str.find('/') {
                    Some(idx) => target_str[idx + 1..].to_string(),
                    None => target_str,
                };
                hardlinks.push((stripped.to_string(), stripped_target));
            }
            _ => {}
        }
    }

    if !hardlinks.is_empty() {
        let link_map: HashMap<&str, &str> = hardlinks
            .iter()
            .map(|(l, t)| (l.as_str(), t.as_str()))
            .collect();

        let mut unresolved: Vec<(String, String)> = Vec::new();

        for (link_path, _) in &hardlinks {
            let mut current = link_path.as_str();
            let mut depth = 0;
            while let Some(&next) = link_map.get(current) {
                current = next;
                depth += 1;
                if depth > 100 {
                    break;
                }
            }
            let src = dest_dir.join(current);
            let dst = dest_dir.join(link_path);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            }
            let _ = std::fs::remove_file(&dst);
            if src.exists() {
                std::fs::copy(&src, &dst).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                if let Ok(meta) = std::fs::metadata(&src) {
                    let _ = std::fs::set_permissions(&dst, meta.permissions());
                }
            } else {
                unresolved.push((link_path.clone(), current.to_string()));
            }
        }

        if !unresolved.is_empty() {
            let mut needed: HashMap<String, Vec<String>> = HashMap::new();
            for (link_path, target_path) in &unresolved {
                needed
                    .entry(target_path.clone())
                    .or_default()
                    .push(link_path.clone());
            }

            let file =
                std::fs::File::open(tar_path).map_err(|e| ClientError::Config(CoreError::Io(e)))?;
            let mut archive2 = tar::Archive::new(file);

            for entry_result in archive2.entries().map_err(|e| {
                ClientError::Config(CoreError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to read tar entries (pass 2): {}", e),
                )))
            })? {
                if needed.is_empty() {
                    break;
                }

                let mut entry = entry_result.map_err(|e| {
                    ClientError::Config(CoreError::Io(std::io::Error::new(
                        e.kind(),
                        format!("Failed to read tar entry (pass 2): {}", e),
                    )))
                })?;

                let raw_path = entry
                    .path()
                    .map_err(|e| {
                        ClientError::Config(CoreError::Io(std::io::Error::new(
                            e.kind(),
                            format!("Invalid path in tar: {}", e),
                        )))
                    })?
                    .to_path_buf();
                let raw_str = raw_path.to_string_lossy().to_string();
                let stripped = match raw_str.find('/') {
                    Some(idx) => &raw_str[idx + 1..],
                    None => continue,
                };

                if let Some(link_paths) = needed.remove(stripped) {
                    let entry_type = entry.header().entry_type();

                    let data_and_mode = if entry_type.is_file() {
                        let mut data = Vec::new();
                        entry
                            .read_to_end(&mut data)
                            .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                        let mode = entry.header().mode().ok();
                        Some((data, mode))
                    } else if entry_type.is_symlink() {
                        if let Ok(Some(target)) =
                            entry.link_name().map(|o| o.map(|p| p.to_path_buf()))
                        {
                            if target.is_absolute() && target.exists() {
                                match std::fs::read(&target) {
                                    Ok(data) => {
                                        let mode = std::fs::metadata(&target).ok().map(|m| {
                                            use std::os::unix::fs::PermissionsExt;
                                            m.permissions().mode()
                                        });
                                        Some((data, mode))
                                    }
                                    Err(_) => None,
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some((data, mode)) = data_and_mode {
                        for link_path in &link_paths {
                            let dst = dest_dir.join(link_path);
                            if let Some(parent) = dst.parent() {
                                std::fs::create_dir_all(parent)
                                    .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                            }
                            let _ = std::fs::remove_file(&dst);
                            std::fs::write(&dst, &data)
                                .map_err(|e| ClientError::Config(CoreError::Io(e)))?;
                            if let Some(m) = mode {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = std::fs::set_permissions(
                                    &dst,
                                    std::fs::Permissions::from_mode(m),
                                );
                            }
                        }
                    } else {
                        needed.insert(stripped.to_string(), link_paths);
                    }
                }
            }
        }
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
