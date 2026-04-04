use std::fs;
use std::io::Write;
use std::path::Path;

pub fn force_remove_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
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
}
