use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn force_remove_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                let _ = fs::set_permissions(
                    entry.path(),
                    std::os::unix::fs::PermissionsExt::from_mode(0o755),
                );
            }
        }
    }
    fs::remove_dir_all(path)
}

pub fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    write_atomic_impl(path, content, true)
}

pub fn write_atomic_nosync(path: &Path, content: &[u8]) -> std::io::Result<()> {
    write_atomic_impl(path, content, false)
}

fn write_atomic_impl(path: &Path, content: &[u8], sync: bool) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        )
    })?;

    fs::create_dir_all(dir)?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    if sync {
        tmp.as_file().sync_all()?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn resolve_link_chain<'a>(
    map: &'a std::collections::HashMap<String, String>,
    start: &'a str,
    max_depth: usize,
) -> &'a str {
    let mut current = start;
    let mut depth = 0;
    while let Some(next) = map.get(current) {
        current = next.as_str();
        depth += 1;
        if depth > max_depth {
            break;
        }
    }
    current
}

pub fn resolve_link_chain_ref<'a>(
    map: &std::collections::HashMap<&'a str, &'a str>,
    start: &'a str,
    max_depth: usize,
) -> &'a str {
    let mut current = start;
    let mut depth = 0;
    while let Some(&next) = map.get(current) {
        current = next;
        depth += 1;
        if depth > max_depth {
            break;
        }
    }
    current
}

pub fn safe_truncate<'a>(s: &'a str, max_len: usize, suffix: &str) -> Cow<'a, str> {
    let suffix_chars = suffix.chars().count();
    if s.chars().count() <= max_len {
        return Cow::Borrowed(s);
    }
    let keep = max_len.saturating_sub(suffix_chars);
    let truncated: String = s.chars().take(keep).collect();
    Cow::Owned(format!("{}{}", truncated, suffix))
}

pub fn safe_truncate_ref(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn path_to_string(p: impl AsRef<std::ffi::OsStr>) -> String {
    p.as_ref().to_string_lossy().into_owned()
}

pub fn format_bytes(bytes: u64, compact: bool) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        if compact {
            format!("{:.1}G", bytes as f64 / GB as f64)
        } else {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        }
    } else if bytes >= MB {
        if compact {
            format!("{:.1}M", bytes as f64 / MB as f64)
        } else {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        }
    } else if bytes >= KB {
        if compact {
            format!("{:.1}K", bytes as f64 / KB as f64)
        } else {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        }
    } else if compact {
        format!("{}B", bytes)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_atomic_creates_file() {
        let dir = tempdir().expect("failed to create tempdir");
        let path = dir.path().join("test.txt");
        write_atomic(&path, b"hello").expect("write_atomic failed");
        assert_eq!(
            fs::read_to_string(&path).expect("failed to read back"),
            "hello"
        );
    }

    #[test]
    fn test_write_atomic_overwrites_existing() {
        let dir = tempdir().expect("failed to create tempdir");
        let path = dir.path().join("test.txt");
        fs::write(&path, "old").expect("failed to write seed file");
        write_atomic(&path, b"new").expect("write_atomic failed");
        assert_eq!(
            fs::read_to_string(&path).expect("failed to read back"),
            "new"
        );
    }

    #[test]
    fn test_write_atomic_creates_parent_dirs() {
        let dir = tempdir().expect("failed to create tempdir");
        let path = dir.path().join("a").join("b").join("test.txt");
        write_atomic(&path, b"nested").expect("write_atomic failed");
        assert_eq!(
            fs::read_to_string(&path).expect("failed to read back"),
            "nested"
        );
    }

    #[test]
    fn test_force_remove_dir_readonly_files() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("failed to create tempdir");
        let target = dir.path().join("readonly-lab");
        let sub = target.join("store").join("pkg");
        fs::create_dir_all(&sub).expect("create dirs");

        let f = sub.join("binary");
        fs::write(&f, b"data").expect("write file");
        fs::set_permissions(&f, fs::Permissions::from_mode(0o444)).expect("set ro file");
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).expect("set ro dir");
        fs::set_permissions(target.join("store"), fs::Permissions::from_mode(0o555))
            .expect("set ro dir");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o555)).expect("set ro dir");

        if std::env::var("USER").as_deref() != Ok("root") {
            assert!(fs::remove_dir_all(&target).is_err());
        }

        force_remove_dir(&target).expect("force_remove_dir failed");
        assert!(!target.exists());
    }

    #[test]
    fn test_force_remove_dir_nonexistent() {
        let dir = tempdir().expect("failed to create tempdir");
        let target = dir.path().join("does-not-exist");
        force_remove_dir(&target).expect("should be a no-op");
    }

    #[test]
    fn test_safe_truncate_ascii() {
        assert_eq!(safe_truncate("hello", 10, ".."), "hello");
        assert_eq!(safe_truncate("hello world", 8, ".."), "hello ..");
        assert_eq!(safe_truncate("ab", 2, ".."), "ab");
        assert_eq!(safe_truncate("abc", 2, ".."), "..");
    }

    #[test]
    fn test_safe_truncate_multibyte() {
        let s = "hello 🌍🌎🌏";
        let result = safe_truncate(s, 8, "..");
        assert!(result.chars().count() <= 8);
        assert!(result.ends_with(".."));
    }

    #[test]
    fn test_safe_truncate_ref_multibyte() {
        let s = "café";
        let result = safe_truncate_ref(s, 4);
        assert!(result.len() <= 4);
        assert!(result.is_char_boundary(result.len()));
    }

    #[test]
    fn test_format_bytes_standard() {
        assert_eq!(format_bytes(500, false), "500 B");
        assert_eq!(format_bytes(1024, false), "1.0 KB");
        assert_eq!(format_bytes(1_048_576, false), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824, false), "1.0 GB");
    }

    #[test]
    fn test_format_bytes_compact() {
        assert_eq!(format_bytes(500, true), "500B");
        assert_eq!(format_bytes(1024, true), "1.0K");
        assert_eq!(format_bytes(1_048_576, true), "1.0M");
        assert_eq!(format_bytes(1_073_741_824, true), "1.0G");
    }
}
