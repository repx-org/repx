pub const ALLOWED_SYSTEM_BINARIES: &[&str] = &[
    "docker", "podman", "sbatch", "squeue", "sinfo", "sacct", "scancel",
];

pub fn allowed_system_binaries() -> &'static [&'static str] {
    ALLOWED_SYSTEM_BINARIES
}

pub fn is_binary_allowed(binary_name: &str) -> bool {
    ALLOWED_SYSTEM_BINARIES.contains(&binary_name)
}

pub fn extract_image_hash(image_tag: &str) -> &str {
    image_tag.split(':').next_back().unwrap_or(image_tag)
}
