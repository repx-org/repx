use std::fmt;

pub const ALLOWED_SYSTEM_BINARIES: &[&str] = &[
    "docker", "podman", "sbatch", "squeue", "sinfo", "sacct", "scancel",
];

pub fn is_binary_allowed(binary_name: &str) -> bool {
    ALLOWED_SYSTEM_BINARIES.contains(&binary_name)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageTag(String);

impl ImageTag {
    pub fn parse(s: impl Into<String>) -> Result<Self, crate::error::ExecutorError> {
        let s = s.into();
        validate_image_identifier(&s)?;
        Ok(ImageTag(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn extract_hash(&self) -> Result<String, crate::error::ExecutorError> {
        let raw = self.0.split(':').next_back().unwrap_or(&self.0);
        validate_image_identifier(raw)?;
        Ok(raw.to_string())
    }
}

impl fmt::Display for ImageTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn validate_image_identifier(id: &str) -> Result<(), crate::error::ExecutorError> {
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
