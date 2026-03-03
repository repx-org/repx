pub const ALLOWED_SYSTEM_BINARIES: &[&str] = &[
    "docker", "podman", "sbatch", "squeue", "sinfo", "sacct", "scancel",
];

pub fn is_binary_allowed(binary_name: &str) -> bool {
    ALLOWED_SYSTEM_BINARIES.contains(&binary_name)
}

pub fn validate_image_identifier(id: &str) -> Result<(), crate::error::ExecutorError> {
    if id.is_empty() {
        return Err(crate::error::ExecutorError::InvalidImage(
            "Image identifier is empty".to_string(),
        ));
    }

    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
    {
        return Err(crate::error::ExecutorError::InvalidImage(format!(
            "Image identifier '{}' contains invalid characters. \
             Only [a-zA-Z0-9._:-] are allowed.",
            id
        )));
    }

    Ok(())
}

pub fn extract_image_hash(image_tag: &str) -> Result<String, crate::error::ExecutorError> {
    let raw = image_tag.split(':').next_back().unwrap_or(image_tag);
    validate_image_identifier(raw)?;
    Ok(raw.to_string())
}
