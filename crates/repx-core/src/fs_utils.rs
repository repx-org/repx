use std::fs;
use std::io::Write;
use std::path::Path;

pub fn write_atomic(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        )
    })?;

    fs::create_dir_all(dir)?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
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
}
