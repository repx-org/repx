use crate::error::AppError;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const ARTIFACTS_DIR: &str = "artifacts";

pub fn has_artifact(base_path: &Path, hash_path: &str) -> bool {
    base_path.join(ARTIFACTS_DIR).join(hash_path).exists()
}

pub fn put_artifact(base_path: &Path, hash_path: &str, content: &[u8]) -> Result<(), AppError> {
    let dest_path = base_path.join(ARTIFACTS_DIR).join(hash_path);

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::PathIo {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    fs::write(&dest_path, content).map_err(|e| AppError::PathIo {
        path: dest_path.clone(),
        source: e,
    })?;

    let relative_path = Path::new(hash_path);
    if relative_path.parent().is_some_and(|p| p.ends_with("bin")) {
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&dest_path)
                .map_err(|e| AppError::PathIo {
                    path: dest_path.clone(),
                    source: e,
                })?
                .permissions();
            // Set rwxr-xr-x permissions
            perms.set_mode(0o755);
            fs::set_permissions(&dest_path, perms).map_err(|e| AppError::PathIo {
                path: dest_path,
                source: e,
            })?;
        }
    }

    Ok(())
}

pub fn get_artifact_path(base_path: &Path, hash_path: &str) -> PathBuf {
    base_path.join(ARTIFACTS_DIR).join(hash_path)
}
