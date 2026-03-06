use crate::errors::CoreError;
use std::path::{Component, Path, PathBuf};

pub fn sanitize_relative_path(untrusted: &str) -> Result<PathBuf, CoreError> {
    if untrusted.is_empty() {
        return Err(CoreError::PathTraversal {
            path: "(empty)".to_string(),
        });
    }

    if untrusted.contains('\0') {
        return Err(CoreError::PathTraversal {
            path: untrusted.to_string(),
        });
    }

    let path = Path::new(untrusted);

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CoreError::PathTraversal {
                    path: untrusted.to_string(),
                });
            }
        }
    }

    Ok(path.to_path_buf())
}

pub fn safe_join(base: &Path, untrusted: &str) -> Result<PathBuf, CoreError> {
    let clean = sanitize_relative_path(untrusted)?;
    Ok(base.join(clean))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_path() {
        assert_eq!(
            sanitize_relative_path("foo/bar/baz").ok(),
            Some(PathBuf::from("foo/bar/baz"))
        );
    }

    #[test]
    fn test_single_component() {
        assert_eq!(
            sanitize_relative_path("abc123").ok(),
            Some(PathBuf::from("abc123"))
        );
    }

    #[test]
    fn test_dot_component_allowed() {
        assert!(sanitize_relative_path("./foo").is_ok());
    }

    #[test]
    fn test_rejects_parent_traversal() {
        assert!(sanitize_relative_path("../escape").is_err());
        assert!(sanitize_relative_path("foo/../../escape").is_err());
        assert!(sanitize_relative_path("..").is_err());
    }

    #[test]
    fn test_rejects_absolute_path() {
        assert!(sanitize_relative_path("/etc/passwd").is_err());
    }

    #[test]
    fn test_rejects_empty() {
        assert!(sanitize_relative_path("").is_err());
    }

    #[test]
    fn test_rejects_null_bytes() {
        assert!(sanitize_relative_path("foo\0bar").is_err());
    }

    #[test]
    fn test_safe_join() {
        let base = Path::new("/store/artifacts");
        assert_eq!(
            safe_join(base, "abc/def").ok(),
            Some(PathBuf::from("/store/artifacts/abc/def"))
        );
        assert!(safe_join(base, "../escape").is_err());
        assert!(safe_join(base, "/absolute").is_err());
    }

    #[test]
    fn test_deeply_nested_traversal() {
        assert!(sanitize_relative_path("a/b/c/../../../..").is_err());
    }

    #[test]
    fn test_path_with_dots_in_names() {
        assert!(sanitize_relative_path("foo.bar/baz.txt").is_ok());
        assert!(sanitize_relative_path("...").is_ok());
    }
}
