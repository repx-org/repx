use repx_executor::{extract_image_hash, is_binary_allowed, validate_image_identifier};

#[test]
fn test_is_binary_allowed_rejects_invalid() {
    assert!(!is_binary_allowed("rm"));
    assert!(!is_binary_allowed("curl"));
    assert!(!is_binary_allowed("wget"));
    assert!(!is_binary_allowed("sh"));
    assert!(!is_binary_allowed("bash"));
    assert!(!is_binary_allowed("/bin/sh"));
}

#[test]
fn test_is_binary_allowed_rejects_path_traversal() {
    assert!(!is_binary_allowed("../docker"));
    assert!(!is_binary_allowed("/usr/bin/docker"));
    assert!(!is_binary_allowed("docker/../rm"));
}

#[test]
fn test_extract_image_hash_with_colon() {
    assert_eq!(
        extract_image_hash("repx-image:abc123def").expect("valid tag"),
        "abc123def"
    );
}

#[test]
fn test_extract_image_hash_no_colon() {
    assert_eq!(
        extract_image_hash("simple-hash").expect("valid tag"),
        "simple-hash"
    );
}

#[test]
fn test_extract_image_hash_empty_after_colon_is_rejected() {
    assert!(extract_image_hash("image:").is_err());
}

#[test]
fn test_extract_image_hash_rejects_path_traversal() {
    assert!(extract_image_hash("image:../../etc/cron.d").is_err());
    assert!(extract_image_hash("../../escape").is_err());
    assert!(extract_image_hash("image:abc/def").is_err());
}

#[test]
fn test_extract_image_hash_with_registry_prefix() {
    assert_eq!(
        extract_image_hash("registry/image:v1.0").expect("valid tag"),
        "v1.0"
    );
}

#[test]
fn test_extract_image_hash_rejects_slash_in_hash() {
    assert!(extract_image_hash("path/with/slash").is_err());
}

#[test]
fn test_validate_image_identifier_valid() {
    assert!(validate_image_identifier("abc123def").is_ok());
    assert!(validate_image_identifier("image-abc123").is_ok());
    assert!(validate_image_identifier("image_abc.123").is_ok());
    assert!(validate_image_identifier("v1.0").is_ok());
    assert!(validate_image_identifier("sha256:abcdef0123456789").is_ok());
}

#[test]
fn test_validate_image_identifier_rejects_empty() {
    assert!(validate_image_identifier("").is_err());
}

#[test]
fn test_validate_image_identifier_rejects_path_separators() {
    assert!(validate_image_identifier("../escape").is_err());
    assert!(validate_image_identifier("a/b").is_err());
    assert!(validate_image_identifier("a\\b").is_err());
    assert!(validate_image_identifier("/etc/passwd").is_err());
}

#[test]
fn test_validate_image_identifier_rejects_special_chars() {
    assert!(validate_image_identifier("image;rm -rf /").is_err());
    assert!(validate_image_identifier("image\0null").is_err());
    assert!(validate_image_identifier("image tag").is_err());
}
