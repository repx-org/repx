use crate::errors::CoreError;
use crate::path_safety::safe_join;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const ARTIFACTS_DIR: &str = "artifacts";

pub fn has_artifact(base_path: &Path, hash_path: &str) -> Result<bool, CoreError> {
    let full = safe_join(&base_path.join(ARTIFACTS_DIR), hash_path)?;
    Ok(full.exists())
}

pub fn put_artifact(base_path: &Path, hash_path: &str, content: &[u8]) -> Result<(), CoreError> {
    let dest_path = safe_join(&base_path.join(ARTIFACTS_DIR), hash_path)?;

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::path_io(parent, e))?;
    }

    fs::write(&dest_path, content).map_err(|e| CoreError::path_io(&dest_path, e))?;

    let relative_path = Path::new(hash_path);
    if relative_path.parent().is_some_and(|p| p.ends_with("bin")) {
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&dest_path)
                .map_err(|e| CoreError::path_io(&dest_path, e))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest_path, perms)
                .map_err(|e| CoreError::path_io(&dest_path, e))?;
        }
    }

    Ok(())
}

pub fn get_artifact_path(base_path: &Path, hash_path: &str) -> Result<PathBuf, CoreError> {
    safe_join(&base_path.join(ARTIFACTS_DIR), hash_path)
}
