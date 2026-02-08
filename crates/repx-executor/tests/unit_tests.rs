use repx_executor::{extract_image_hash, is_binary_allowed};

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
    assert_eq!(extract_image_hash("repx-image:abc123def"), "abc123def");
}

#[test]
fn test_extract_image_hash_multiple_colons() {
    assert_eq!(extract_image_hash("registry:5000/image:v1.0"), "v1.0");
}

#[test]
fn test_extract_image_hash_no_colon() {
    assert_eq!(extract_image_hash("simple-hash"), "simple-hash");
}

#[test]
fn test_extract_image_hash_empty_after_colon() {
    assert_eq!(extract_image_hash("image:"), "");
}
