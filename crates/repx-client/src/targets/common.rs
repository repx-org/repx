use crate::error::{ClientError, Result};
use repx_core::errors::ConfigError;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn parse_image_hash(filename: &str) -> &str {
    if let Some(stripped) = filename.strip_suffix(".tar.gz") {
        stripped
    } else if let Some(stripped) = filename.strip_suffix(".tar") {
        stripped
    } else {
        Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename)
    }
}

pub fn generate_gc_link_name(lab_hash: &str) -> String {
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    format!("{}_{}", timestamp, lab_hash)
}

pub fn extract_image_to_cache(
    image_path: &Path,
    cache_root: &Path,
    tar_tool: &Path,
) -> Result<PathBuf> {
    let image_filename = image_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| ClientError::InvalidPath {
            path: image_path.to_path_buf(),
            reason: "Image path has no filename".to_string(),
        })?;

    let image_hash_name = parse_image_hash(image_filename);
    let images_cache = cache_root.join("images");
    let image_extract_dir = images_cache.join(image_hash_name);

    if image_extract_dir.exists() {
        return Ok(image_extract_dir);
    }

    if image_path.is_dir() {
        tracing::info!(
            "Image is already an exploded OCI layout, copying to cache: {}",
            image_extract_dir.display()
        );

        fs_err::create_dir_all(&images_cache).map_err(ConfigError::Io)?;

        let mut cp_cmd = Command::new("cp");
        cp_cmd.arg("-r").arg(image_path).arg(&image_extract_dir);

        repx_core::logging::log_and_print_command(&cp_cmd);
        let cp_output = cp_cmd.output().map_err(ConfigError::Io)?;

        if !cp_output.status.success() {
            let stderr = String::from_utf8_lossy(&cp_output.stderr);
            return Err(ClientError::Config(ConfigError::General(format!(
                "Failed to copy image directory {}: {}",
                image_path.display(),
                stderr
            ))));
        }

        return Ok(image_extract_dir);
    }

    tracing::info!(
        "Extracting image tarball to local cache: {}",
        image_extract_dir.display()
    );

    fs_err::create_dir_all(&image_extract_dir).map_err(ConfigError::Io)?;

    let mut tar_cmd = Command::new(tar_tool);
    tar_cmd
        .arg("-xf")
        .arg(image_path)
        .arg("-C")
        .arg(&image_extract_dir);

    repx_core::logging::log_and_print_command(&tar_cmd);
    let tar_output = tar_cmd.output().map_err(ConfigError::Io)?;

    if !tar_output.status.success() {
        let stderr = String::from_utf8_lossy(&tar_output.stderr);
        let _ = fs_err::remove_dir_all(&image_extract_dir);
        return Err(ClientError::Config(ConfigError::General(format!(
            "Failed to extract image tarball {}: {}",
            image_path.display(),
            stderr
        ))));
    }

    Ok(image_extract_dir)
}

pub fn restructure_layers_for_dedup(image_extract_dir: &Path, layers_cache: &Path) -> Result<()> {
    fs_err::create_dir_all(layers_cache).map_err(ConfigError::Io)?;

    for entry in fs_err::read_dir(image_extract_dir).map_err(ConfigError::Io)? {
        let entry = entry.map_err(ConfigError::Io)?;
        let path = entry.path();

        if path.is_dir() {
            let dirname = entry.file_name();
            if path.join("layer.tar").exists() {
                let layer_cache_path = layers_cache.join(&dirname);

                if !layer_cache_path.exists() {
                    fs_err::rename(&path, &layer_cache_path).map_err(ConfigError::Io)?;
                } else {
                    fs_err::remove_dir_all(&path).map_err(ConfigError::Io)?;
                }

                let relative_target = PathBuf::from("../../layers").join(&dirname);
                std::os::unix::fs::symlink(&relative_target, &path).map_err(ConfigError::Io)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn test_shell_quote_with_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_quote_with_spaces() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    fn test_parse_image_hash_tar_gz() {
        assert_eq!(parse_image_hash("image-abc123.tar.gz"), "image-abc123");
    }

    #[test]
    fn test_parse_image_hash_tar() {
        assert_eq!(parse_image_hash("image-abc123.tar"), "image-abc123");
    }

    #[test]
    fn test_parse_image_hash_no_extension() {
        assert_eq!(parse_image_hash("image-abc123"), "image-abc123");
    }

    #[test]
    fn test_generate_gc_link_name() {
        let link_name = generate_gc_link_name("abc123");
        assert!(link_name.ends_with("_abc123"));
        assert!(link_name.contains("-"));
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ManifestEntry {
    #[serde(rename = "Layers")]
    pub layers: Vec<String>,
}

pub fn get_image_manifest(image_path: &Path, tar_tool: &Path) -> Result<Vec<String>> {
    let mut tar_list_cmd = Command::new(tar_tool);
    tar_list_cmd.arg("-tf").arg(image_path);

    let list_output = tar_list_cmd.output().map_err(ConfigError::Io)?;

    if !list_output.status.success() {
        return Err(ClientError::Config(ConfigError::General(format!(
            "Failed to list tar content {}: {}",
            image_path.display(),
            String::from_utf8_lossy(&list_output.stderr)
        ))));
    }

    let file_list = String::from_utf8_lossy(&list_output.stdout);
    let manifest_path = file_list
        .lines()
        .find(|line| line.trim() == "manifest.json" || line.trim().ends_with("/manifest.json"))
        .ok_or_else(|| {
            ClientError::Config(ConfigError::General(format!(
                "manifest.json not found in {}: manifest.json missing from tar listing",
                image_path.display()
            )))
        })?;

    let mut tar_extract_cmd = Command::new(tar_tool);
    tar_extract_cmd
        .arg("-xf")
        .arg(image_path)
        .arg(manifest_path)
        .arg("-O");

    let extract_output = tar_extract_cmd.output().map_err(ConfigError::Io)?;

    if !extract_output.status.success() {
        return Err(ClientError::Config(ConfigError::General(format!(
            "Failed to extract manifest from {}: {}",
            image_path.display(),
            String::from_utf8_lossy(&extract_output.stderr)
        ))));
    }

    let manifest: Vec<ManifestEntry> = serde_json::from_slice(&extract_output.stdout)
        .map_err(|e| ClientError::Config(ConfigError::Json(e)))?;

    if manifest.is_empty() {
        return Err(ClientError::Config(ConfigError::General(format!(
            "Empty manifest in {}",
            image_path.display()
        ))));
    }

    Ok(manifest[0].layers.clone())
}

pub fn extract_layer_to_cache(
    image_path: &Path,
    layer_path_in_tar: &str,
    layer_hash: &str,
    layers_cache: &Path,
    tar_tool: &Path,
) -> Result<()> {
    let layer_dest_dir = layers_cache.join(layer_hash);

    if layer_dest_dir.exists() {
        return Ok(());
    }

    let temp_extract_dir = layers_cache.join(format!(".tmp_{}", layer_hash));
    if temp_extract_dir.exists() {
        let _ = fs_err::remove_dir_all(&temp_extract_dir);
    }
    fs_err::create_dir_all(&temp_extract_dir).map_err(ConfigError::Io)?;

    tracing::info!(
        "Extracting layer {} from {} to cache",
        layer_hash,
        image_path.display()
    );

    let mut tar_cmd = Command::new(tar_tool);
    tar_cmd
        .arg("-xf")
        .arg(image_path)
        .arg(layer_path_in_tar)
        .arg("-O");

    let output = tar_cmd.output().map_err(ConfigError::Io)?;

    if !output.status.success() {
        let _ = fs_err::remove_dir_all(&temp_extract_dir);
        return Err(ClientError::Config(ConfigError::General(format!(
            "Failed to extract layer {} from {}: {}",
            layer_path_in_tar,
            image_path.display(),
            String::from_utf8_lossy(&output.stderr)
        ))));
    }

    let layer_tar_path = temp_extract_dir.join("layer.tar");
    fs_err::write(&layer_tar_path, &output.stdout).map_err(ConfigError::Io)?;

    fs_err::rename(&temp_extract_dir, &layer_dest_dir).map_err(ConfigError::Io)?;

    Ok(())
}
