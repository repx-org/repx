use repx_core::model::JobId;
use repx_executor::{ExecutionRequest, Executor, ExecutorError, Runtime};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn create_test_request(base_path: PathBuf) -> ExecutionRequest {
    ExecutionRequest {
        job_id: JobId("test-job-123".to_string()),
        runtime: Runtime::Native,
        base_path: base_path.clone(),
        node_local_path: None,
        job_package_path: base_path.join("jobs/test-job"),
        inputs_json_path: base_path.join("inputs.json"),
        user_out_dir: base_path.join("outputs/out"),
        repx_out_dir: base_path.join("outputs/repx"),
        host_tools_bin_dir: None,
        mount_host_paths: false,
        mount_paths: vec![],
    }
}

fn create_test_request_with_host_tools(
    base_path: PathBuf,
    host_tools: PathBuf,
) -> ExecutionRequest {
    ExecutionRequest {
        job_id: JobId("test-job-456".to_string()),
        runtime: Runtime::Native,
        base_path: base_path.clone(),
        node_local_path: None,
        job_package_path: base_path.join("jobs/test-job"),
        inputs_json_path: base_path.join("inputs.json"),
        user_out_dir: base_path.join("outputs/out"),
        repx_out_dir: base_path.join("outputs/repx"),
        host_tools_bin_dir: Some(host_tools),
        mount_host_paths: false,
        mount_paths: vec![],
    }
}

#[test]
fn test_executor_with_different_runtimes() {
    let temp = tempdir().unwrap();

    let mut request = create_test_request(temp.path().to_path_buf());
    request.runtime = Runtime::Docker {
        image_tag: "test:latest".to_string(),
    };
    let executor = Executor::new(request);
    assert!(matches!(executor.request.runtime, Runtime::Docker { .. }));

    let mut request2 = create_test_request(temp.path().to_path_buf());
    request2.runtime = Runtime::Podman {
        image_tag: "test:v1".to_string(),
    };
    let executor2 = Executor::new(request2);
    assert!(matches!(executor2.request.runtime, Runtime::Podman { .. }));
}

#[test]
fn test_execution_request_with_mount_paths() {
    let temp = tempdir().unwrap();
    let mut request = create_test_request(temp.path().to_path_buf());
    request.mount_paths = vec!["/data".to_string(), "/scratch".to_string()];

    assert_eq!(request.mount_paths.len(), 2);
    assert!(request.mount_paths.contains(&"/data".to_string()));
}

#[test]
fn test_find_image_file_in_images_dir() {
    let temp = tempdir().unwrap();
    let images_dir = temp.path().join("artifacts/images");
    fs::create_dir_all(&images_dir).unwrap();

    let image_file = images_dir.join("my-image.tar.gz");
    fs::write(&image_file, "fake image content").unwrap();

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("my-image");
    assert!(result.is_some());
    assert_eq!(result.unwrap(), image_file);
}

#[test]
fn test_find_image_file_in_image_dir() {
    let temp = tempdir().unwrap();
    let image_dir = temp.path().join("artifacts/image");
    fs::create_dir_all(&image_dir).unwrap();

    let image_file = image_dir.join("container.tar");
    fs::write(&image_file, "fake image").unwrap();

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("container");
    assert!(result.is_some());
    assert_eq!(result.unwrap(), image_file);
}

#[test]
fn test_find_image_file_exact_match() {
    let temp = tempdir().unwrap();
    let images_dir = temp.path().join("artifacts/images");
    fs::create_dir_all(&images_dir).unwrap();

    let image_file = images_dir.join("exact-name");
    fs::write(&image_file, "content").unwrap();

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("exact-name");
    assert!(result.is_some());
}

#[test]
fn test_find_image_file_not_found() {
    let temp = tempdir().unwrap();
    let images_dir = temp.path().join("artifacts/images");
    fs::create_dir_all(&images_dir).unwrap();

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("nonexistent");
    assert!(result.is_none());
}

#[test]
fn test_find_image_file_no_artifacts_dir() {
    let temp = tempdir().unwrap();
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("any-image");
    assert!(result.is_none());
}

#[test]
fn test_get_host_tool_path_no_dir_configured() {
    let temp = tempdir().unwrap();
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.get_host_tool_path("some-tool");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ExecutorError::Config(_)));
}

#[test]
fn test_get_host_tool_path_tool_exists() {
    let temp = tempdir().unwrap();
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).unwrap();

    let tool_path = host_tools.join("my-tool");
    fs::write(&tool_path, "#!/bin/sh\necho hello").unwrap();

    let request =
        create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools.clone());
    let executor = Executor::new(request);

    let result = executor.get_host_tool_path("my-tool");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), tool_path);
}

#[test]
fn test_get_host_tool_path_tool_not_found() {
    let temp = tempdir().unwrap();
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).unwrap();

    let request = create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools);
    let executor = Executor::new(request);

    let result = executor.get_host_tool_path("nonexistent-tool");
    assert!(result.is_err());
}

#[test]
fn test_calculate_restricted_path_empty_with_no_host_tools() {
    let temp = tempdir().unwrap();
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let path = executor.calculate_restricted_path(&[]);
    assert_eq!(path, std::ffi::OsString::from(""));
}

#[test]
fn test_calculate_restricted_path_with_host_tools() {
    let temp = tempdir().unwrap();
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).unwrap();

    let request =
        create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools.clone());
    let executor = Executor::new(request);

    let path = executor.calculate_restricted_path(&[]);
    assert!(path.to_string_lossy().contains("host-tools"));
}

#[test]
fn test_calculate_restricted_path_rejects_disallowed_binary() {
    let temp = tempdir().unwrap();
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let path = executor.calculate_restricted_path(&["rm", "curl"]);
    assert_eq!(path, std::ffi::OsString::from(""));
}

#[test]
fn test_build_native_command_basic() {
    let temp = tempdir().unwrap();
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/path/to/script.sh");
    let args = vec!["arg1".to_string(), "arg2".to_string()];
    let cmd = executor.build_native_command(&script_path, &args);

    let std_cmd = cmd.as_std();
    assert_eq!(std_cmd.get_program(), "/path/to/script.sh");

    let collected_args: Vec<_> = std_cmd.get_args().collect();
    assert_eq!(collected_args.len(), 2);
}

#[test]
fn test_build_native_command_with_host_tools() {
    let temp = tempdir().unwrap();
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).unwrap();

    let request =
        create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools.clone());
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/script.sh");
    let cmd = executor.build_native_command(&script_path, &[]);

    let envs: std::collections::HashMap<_, _> = cmd.as_std().get_envs().collect();
    let path_env = envs.get(std::ffi::OsStr::new("PATH"));
    assert!(path_env.is_some());
    let path_val = path_env.unwrap().unwrap().to_string_lossy();
    assert!(path_val.contains("host-tools"));
}

#[test]
fn test_build_native_command_empty_args() {
    let temp = tempdir().unwrap();
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/script.sh");
    let cmd = executor.build_native_command(&script_path, &[]);

    let std_cmd = cmd.as_std();
    assert_eq!(std_cmd.get_args().count(), 0);
}

#[test]
fn test_executor_error_io_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err = ExecutorError::Io(io_err);
    let display = format!("{}", err);
    assert!(display.contains("I/O error"));
}

#[test]
fn test_executor_error_command_failed_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let err = ExecutorError::CommandFailed {
        command: "docker run".to_string(),
        source: io_err,
    };
    let display = format!("{}", err);
    assert!(display.contains("docker run"));
    assert!(display.contains("Failed to execute"));
}

#[test]
fn test_executor_error_script_failed_display() {
    let err = ExecutorError::ScriptFailed {
        script: "/path/to/script.sh".to_string(),
        code: 127,
        stderr: "command not found".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("script.sh"));
    assert!(display.contains("127"));
    assert!(display.contains("command not found"));
}

#[test]
fn test_executor_error_image_tag_missing() {
    let err = ExecutorError::ImageTagMissing;
    let display = format!("{}", err);
    assert!(display.contains("image tag"));
}

#[test]
fn test_executor_error_security_violation() {
    let err = ExecutorError::SecurityViolation("rm".to_string());
    let display = format!("{}", err);
    assert!(display.contains("Security violation"));
    assert!(display.contains("rm"));
    assert!(display.contains("allowlist"));
}

#[tokio::test]
async fn test_ensure_bwrap_rootfs_extracted_from_directory() {
    let temp = tempdir().unwrap();
    let base_path = temp.path().to_path_buf();
    let images_dir = base_path.join("artifacts/images");
    fs::create_dir_all(&images_dir).unwrap();

    let host_tools = base_path.join("host-tools/bin");
    fs::create_dir_all(&host_tools).unwrap();
    let tar_path = host_tools.join("tar");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&tar_path, "#!/bin/sh\nexit 0").unwrap();
        let mut perms = fs::metadata(&tar_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tar_path, perms).unwrap();
    }

    let image_tag = "my-image:v1";
    let image_hash = "v1";
    let image_dir = images_dir.join(image_tag);
    fs::create_dir_all(&image_dir).unwrap();

    let manifest_json = r#"[{"Layers": ["layer1/layer.tar"]}]"#;
    fs::write(image_dir.join("manifest.json"), manifest_json).unwrap();

    let layer_dir = image_dir.join("layer1");
    fs::create_dir_all(&layer_dir).unwrap();
    fs::write(layer_dir.join("layer.tar"), "dummy tar content").unwrap();

    let mut request = create_test_request_with_host_tools(base_path.clone(), host_tools);
    request.runtime = Runtime::Bwrap {
        image_tag: image_tag.to_string(),
    };
    let executor = Executor::new(request);

    let result = executor.ensure_bwrap_rootfs_extracted(image_tag).await;

    assert!(
        result.is_ok(),
        "Extraction should succeed with directory input. Error: {:?}",
        result.err()
    );
    let rootfs = result.unwrap();
    assert!(rootfs.ends_with(format!("cache/images/{}/rootfs", image_hash)));
    assert!(rootfs.exists());
    assert!(base_path
        .join("cache/images")
        .join(image_hash)
        .join("SUCCESS")
        .exists());
}

#[tokio::test]
async fn test_build_command_for_script_native_runtime() {
    let temp = tempdir().unwrap();
    let base_path = temp.path().to_path_buf();
    let repx_out_dir = base_path.join("outputs/repx");
    fs::create_dir_all(&repx_out_dir).unwrap();

    let mut request = create_test_request(base_path);
    request.runtime = Runtime::Native;
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/test/script.sh");
    let args = vec!["--flag".to_string(), "value".to_string()];

    let result = executor.build_command_for_script(&script_path, &args).await;
    assert!(result.is_ok(), "Native command build should succeed");

    let cmd = result.unwrap();
    let std_cmd = cmd.as_std();
    assert_eq!(std_cmd.get_program(), "/test/script.sh");

    let collected_args: Vec<_> = std_cmd.get_args().collect();
    assert_eq!(collected_args.len(), 2);
}

#[tokio::test]
async fn test_build_command_for_script_with_special_chars_in_args() {
    let temp = tempdir().unwrap();
    let base_path = temp.path().to_path_buf();
    let repx_out_dir = base_path.join("outputs/repx");
    fs::create_dir_all(&repx_out_dir).unwrap();

    let mut request = create_test_request(base_path);
    request.runtime = Runtime::Native;
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/test/script.sh");
    let args = vec![
        "arg with spaces".to_string(),
        "--key=value".to_string(),
        "path/to/file.txt".to_string(),
    ];

    let result = executor.build_command_for_script(&script_path, &args).await;
    assert!(result.is_ok());

    let cmd = result.unwrap();
    let collected_args: Vec<_> = cmd.as_std().get_args().collect();
    assert_eq!(collected_args.len(), 3);
}

#[tokio::test]
async fn test_ensure_bwrap_rootfs_fails_if_file() {
    let temp = tempdir().unwrap();
    let base_path = temp.path().to_path_buf();
    let images_dir = base_path.join("artifacts/images");
    fs::create_dir_all(&images_dir).unwrap();

    let host_tools = base_path.join("host-tools/bin");
    fs::create_dir_all(&host_tools).unwrap();
    let tar_path = host_tools.join("tar");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&tar_path, "#!/bin/sh\nexit 0").unwrap();
        let mut perms = fs::metadata(&tar_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tar_path, perms).unwrap();
    }

    let image_tag = "my-image:v1";
    let image_file = images_dir.join(image_tag);
    fs::write(&image_file, "i am a file").unwrap();

    let mut request = create_test_request_with_host_tools(base_path.clone(), host_tools);
    request.runtime = Runtime::Bwrap {
        image_tag: image_tag.to_string(),
    };
    let executor = Executor::new(request);

    let result = executor.ensure_bwrap_rootfs_extracted(image_tag).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        format!("{}", err).contains("must be a directory"),
        "Error was: {}",
        err
    );
}
