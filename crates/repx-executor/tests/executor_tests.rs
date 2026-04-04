#![allow(clippy::expect_used)]

use repx_core::model::{JobId, MountPolicy};
use repx_executor::{
    CancellationToken, ExecutionRequest, Executor, ExecutorError, ImageTag, Runtime,
};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn create_test_request(base_path: PathBuf) -> ExecutionRequest {
    ExecutionRequest {
        job_id: JobId::from("test-job-123"),
        runtime: Runtime::Native,
        base_path: base_path.clone(),
        node_local_path: None,
        local_artifacts_path: None,
        job_package_path: base_path.join("jobs/test-job"),
        inputs_json_path: base_path.join("inputs.json"),
        user_out_dir: base_path.join("outputs/out"),
        repx_out_dir: base_path.join("outputs/repx"),
        host_tools_bin_dir: None,
        mount_policy: MountPolicy::Isolated,
        inputs_data: None,
        parameters_data: None,
    }
}

fn create_test_request_with_host_tools(
    base_path: PathBuf,
    host_tools: PathBuf,
) -> ExecutionRequest {
    ExecutionRequest {
        job_id: JobId::from("test-job-456"),
        runtime: Runtime::Native,
        base_path: base_path.clone(),
        node_local_path: None,
        local_artifacts_path: None,
        job_package_path: base_path.join("jobs/test-job"),
        inputs_json_path: base_path.join("inputs.json"),
        user_out_dir: base_path.join("outputs/out"),
        repx_out_dir: base_path.join("outputs/repx"),
        host_tools_bin_dir: Some(host_tools),
        mount_policy: MountPolicy::Isolated,
        inputs_data: None,
        parameters_data: None,
    }
}

#[test]
fn test_executor_with_different_runtimes() {
    let temp = tempdir().expect("tempdir creation must succeed");

    let mut request = create_test_request(temp.path().to_path_buf());
    request.runtime = Runtime::Docker {
        image_tag: ImageTag::parse("test:latest").expect("valid image tag"),
    };
    let executor = Executor::new(request);
    assert!(matches!(executor.request.runtime, Runtime::Docker { .. }));

    let mut request2 = create_test_request(temp.path().to_path_buf());
    request2.runtime = Runtime::Podman {
        image_tag: ImageTag::parse("test:v1").expect("valid image tag"),
    };
    let executor2 = Executor::new(request2);
    assert!(matches!(executor2.request.runtime, Runtime::Podman { .. }));
}

#[test]
fn test_execution_request_with_mount_paths() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let mut request = create_test_request(temp.path().to_path_buf());
    request.mount_policy =
        MountPolicy::SpecificPaths(vec!["/data".to_string(), "/scratch".to_string()]);

    assert_eq!(request.mount_policy.specific_paths().len(), 2);
    assert!(request
        .mount_policy
        .specific_paths()
        .contains(&"/data".to_string()));
}

#[tokio::test]
async fn test_find_image_file_in_images_dir() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let images_dir = temp.path().join("artifacts/images");
    fs::create_dir_all(&images_dir).expect("dir creation must succeed");

    let image_file = images_dir.join("my-image.tar.gz");
    fs::write(&image_file, "fake image content").expect("file write must succeed");

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("my-image").await;
    assert!(result.is_some());
    assert_eq!(result.expect("result must be Ok"), image_file);
}

#[tokio::test]
async fn test_find_image_file_in_image_dir() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let image_dir = temp.path().join("artifacts/image");
    fs::create_dir_all(&image_dir).expect("dir creation must succeed");

    let image_file = image_dir.join("container.tar");
    fs::write(&image_file, "fake image").expect("file write must succeed");

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("container").await;
    assert!(result.is_some());
    assert_eq!(result.expect("result must be Ok"), image_file);
}

#[tokio::test]
async fn test_find_image_file_exact_match() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let images_dir = temp.path().join("artifacts/images");
    fs::create_dir_all(&images_dir).expect("dir creation must succeed");

    let image_file = images_dir.join("exact-name");
    fs::write(&image_file, "content").expect("file write must succeed");

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("exact-name").await;
    assert!(result.is_some());
}

#[tokio::test]
async fn test_find_image_file_not_found() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let images_dir = temp.path().join("artifacts/images");
    fs::create_dir_all(&images_dir).expect("dir creation must succeed");

    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("nonexistent").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_find_image_file_no_artifacts_dir() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.find_image_file("any-image").await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_host_tool_path_no_dir_configured() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let result = executor.get_host_tool_path("some-tool").await;
    assert!(result.is_err());
    let err = result.expect_err("result must be Err");
    assert!(matches!(err, ExecutorError::Config(_)));
}

#[tokio::test]
async fn test_get_host_tool_path_tool_exists() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).expect("dir creation must succeed");

    let tool_path = host_tools.join("my-tool");
    fs::write(&tool_path, "#!/bin/sh\necho hello").expect("file write must succeed");

    let request =
        create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools.clone());
    let executor = Executor::new(request);

    let result = executor.get_host_tool_path("my-tool").await;
    assert!(result.is_ok());
    assert_eq!(result.expect("result must be Ok"), tool_path);
}

#[tokio::test]
async fn test_get_host_tool_path_tool_not_found() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).expect("dir creation must succeed");

    let request = create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools);
    let executor = Executor::new(request);

    let result = executor.get_host_tool_path("nonexistent-tool").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calculate_restricted_path_empty_with_no_host_tools() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let path = executor.calculate_restricted_path(&[]).await;
    assert_eq!(path, std::ffi::OsString::from(""));
}

#[tokio::test]
async fn test_calculate_restricted_path_with_host_tools() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).expect("dir creation must succeed");

    let request =
        create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools.clone());
    let executor = Executor::new(request);

    let path = executor.calculate_restricted_path(&[]).await;
    assert!(path.to_string_lossy().contains("host-tools"));
}

#[tokio::test]
async fn test_calculate_restricted_path_rejects_disallowed_binary() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let path = executor.calculate_restricted_path(&["rm", "curl"]).await;
    assert_eq!(path, std::ffi::OsString::from(""));
}

#[test]
fn test_build_native_command_basic() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/path/to/script.sh");
    let args = vec!["arg1".to_string(), "arg2".to_string()];
    let cmd = executor
        .build_native_command(&script_path, &args)
        .expect("build command");

    let std_cmd = cmd.as_std();
    assert_eq!(std_cmd.get_program(), "/path/to/script.sh");

    let collected_args: Vec<_> = std_cmd.get_args().collect();
    assert_eq!(collected_args.len(), 2);
}

#[test]
fn test_build_native_command_with_host_tools() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let host_tools = temp.path().join("host-tools/bin");
    fs::create_dir_all(&host_tools).expect("dir creation must succeed");

    let request =
        create_test_request_with_host_tools(temp.path().to_path_buf(), host_tools.clone());
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/script.sh");
    let cmd = executor
        .build_native_command(&script_path, &[])
        .expect("build command");

    let envs: std::collections::HashMap<_, _> = cmd.as_std().get_envs().collect();
    let path_env = envs.get(std::ffi::OsStr::new("PATH"));
    assert!(path_env.is_some());
    let path_val = path_env
        .expect("PATH env must be present")
        .expect("PATH value must be set")
        .to_string_lossy();
    assert!(path_val.contains("host-tools"));
}

#[test]
fn test_build_native_command_empty_args() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let request = create_test_request(temp.path().to_path_buf());
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/script.sh");
    let cmd = executor
        .build_native_command(&script_path, &[])
        .expect("build command");

    let std_cmd = cmd.as_std();
    assert_eq!(std_cmd.get_args().count(), 0);
}

#[test]
fn test_executor_error_io_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err = ExecutorError::Io {
        operation: "read",
        path: std::path::PathBuf::from("/tmp/missing"),
        source: io_err,
    };
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

#[tokio::test]
async fn test_ensure_bwrap_rootfs_extracted_from_directory() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let base_path = temp.path().to_path_buf();
    let images_dir = base_path.join("artifacts/images");
    fs::create_dir_all(&images_dir).expect("dir creation must succeed");

    let host_tools = base_path.join("host-tools/bin");
    fs::create_dir_all(&host_tools).expect("dir creation must succeed");
    let tar_path = host_tools.join("tar");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&tar_path, "#!/bin/sh\nexit 0").expect("file write must succeed");
        let mut perms = fs::metadata(&tar_path)
            .expect("metadata read must succeed")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tar_path, perms).expect("set permissions must succeed");
    }

    let image_tag_str = "my-image:v1";
    let image_hash = "v1";
    let image_dir = images_dir.join(image_tag_str);
    fs::create_dir_all(&image_dir).expect("dir creation must succeed");

    let manifest_json = r#"[{"Layers": ["layer1/layer.tar"]}]"#;
    fs::write(image_dir.join("manifest.json"), manifest_json).expect("file write must succeed");

    let layer_dir = image_dir.join("layer1");
    fs::create_dir_all(&layer_dir).expect("dir creation must succeed");
    fs::write(layer_dir.join("layer.tar"), "dummy tar content").expect("file write must succeed");

    let mut request = create_test_request_with_host_tools(base_path.clone(), host_tools);
    request.runtime = Runtime::Bwrap {
        image_tag: ImageTag::parse(image_tag_str).expect("valid image tag"),
    };
    let executor = Executor::new(request);

    let result = executor.ensure_bwrap_rootfs_extracted(image_tag_str).await;

    assert!(
        result.is_ok(),
        "Extraction should succeed with directory input. Error: {:?}",
        result.err()
    );
    let rootfs = result.expect("result must be Ok");
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
    let temp = tempdir().expect("tempdir creation must succeed");
    let base_path = temp.path().to_path_buf();
    let repx_out_dir = base_path.join("outputs/repx");
    fs::create_dir_all(&repx_out_dir).expect("dir creation must succeed");

    let mut request = create_test_request(base_path);
    request.runtime = Runtime::Native;
    let executor = Executor::new(request);

    let script_path = PathBuf::from("/test/script.sh");
    let args = vec!["--flag".to_string(), "value".to_string()];

    let result = executor.build_command_for_script(&script_path, &args).await;
    assert!(result.is_ok(), "Native command build should succeed");

    let cmd = result.expect("result must be Ok");
    let std_cmd = cmd.as_std();
    assert_eq!(std_cmd.get_program(), "/test/script.sh");

    let collected_args: Vec<_> = std_cmd.get_args().collect();
    assert_eq!(collected_args.len(), 2);
}

#[tokio::test]
async fn test_build_command_for_script_with_special_chars_in_args() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let base_path = temp.path().to_path_buf();
    let repx_out_dir = base_path.join("outputs/repx");
    fs::create_dir_all(&repx_out_dir).expect("dir creation must succeed");

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

    let cmd = result.expect("result must be Ok");
    let collected_args: Vec<_> = cmd.as_std().get_args().collect();
    assert_eq!(collected_args.len(), 3);
}

#[tokio::test]
async fn test_ensure_bwrap_rootfs_fails_if_file() {
    let temp = tempdir().expect("tempdir creation must succeed");
    let base_path = temp.path().to_path_buf();
    let images_dir = base_path.join("artifacts/images");
    fs::create_dir_all(&images_dir).expect("dir creation must succeed");

    let host_tools = base_path.join("host-tools/bin");
    fs::create_dir_all(&host_tools).expect("dir creation must succeed");
    let tar_path = host_tools.join("tar");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(&tar_path, "#!/bin/sh\nexit 0").expect("file write must succeed");
        let mut perms = fs::metadata(&tar_path)
            .expect("metadata read must succeed")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tar_path, perms).expect("set permissions must succeed");
    }

    let image_tag_str = "my-image:v1";
    let image_file = images_dir.join(image_tag_str);
    fs::write(&image_file, "i am a file").expect("file write must succeed");

    let mut request = create_test_request_with_host_tools(base_path.clone(), host_tools);
    request.runtime = Runtime::Bwrap {
        image_tag: ImageTag::parse(image_tag_str).expect("valid image tag"),
    };
    let executor = Executor::new(request);

    let result = executor.ensure_bwrap_rootfs_extracted(image_tag_str).await;

    assert!(result.is_err());
    let err = result.expect_err("result must be Err");
    assert!(
        format!("{}", err).contains("must be a directory"),
        "Error was: {}",
        err
    );
}

fn create_runnable_request(temp: &tempfile::TempDir) -> (ExecutionRequest, PathBuf) {
    let base = temp.path().to_path_buf();
    let repx_out = base.join("outputs/repx");
    let user_out = base.join("outputs/out");
    let job_pkg = base.join("jobs/test-job");
    fs::create_dir_all(&repx_out).expect("create repx_out_dir");
    fs::create_dir_all(&user_out).expect("create user_out_dir");
    fs::create_dir_all(&job_pkg).expect("create job_package_path");

    let request = ExecutionRequest {
        job_id: JobId::from("cancel-test-job"),
        runtime: Runtime::Native,
        base_path: base.clone(),
        node_local_path: None,
        local_artifacts_path: None,
        job_package_path: job_pkg,
        inputs_json_path: base.join("inputs.json"),
        user_out_dir: user_out,
        repx_out_dir: repx_out,
        host_tools_bin_dir: None,
        mount_policy: MountPolicy::Isolated,
        inputs_data: None,
        parameters_data: None,
    };
    (request, base)
}

#[cfg(unix)]
fn write_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{}", body)).expect("write script");
    let mut perms = fs::metadata(&path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod");
    path
}

#[cfg(unix)]
#[tokio::test]
async fn test_execute_script_cancelled_kills_child() {
    let temp = tempdir().expect("tempdir");
    let (request, base) = create_runnable_request(&temp);
    let script = write_script(&base, "sleep.sh", "sleep 300");
    let mut executor = Executor::new(request);

    let token = CancellationToken::new();
    let token_clone = token.clone();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        token_clone.cancel();
    });

    let result = executor.execute_script(&script, &[], &token).await;

    assert!(result.is_err(), "should return an error on cancellation");
    let err = result.expect_err("must be Err");
    assert!(
        matches!(err, ExecutorError::Cancelled { .. }),
        "expected Cancelled, got: {err}",
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_execute_script_pre_cancelled_token_returns_immediately() {
    let temp = tempdir().expect("tempdir");
    let (request, base) = create_runnable_request(&temp);

    let script = write_script(&base, "long.sh", "sleep 300");
    let mut executor = Executor::new(request);

    let token = CancellationToken::new();
    token.cancel();

    let start = std::time::Instant::now();
    let result = executor.execute_script(&script, &[], &token).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "pre-cancelled token should error");
    assert!(
        matches!(
            result.expect_err("must be Err"),
            ExecutorError::Cancelled { .. }
        ),
        "expected Cancelled variant",
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "pre-cancelled token should not wait; took {:?}",
        elapsed,
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_execute_script_succeeds_with_live_token() {
    let temp = tempdir().expect("tempdir");
    let (request, base) = create_runnable_request(&temp);

    let script = write_script(&base, "ok.sh", "exit 0");
    let mut executor = Executor::new(request);

    let token = CancellationToken::new();
    let result = executor.execute_script(&script, &[], &token).await;

    assert!(
        result.is_ok(),
        "script should succeed with a live token: {:?}",
        result.err(),
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_execute_script_failure_not_masked_by_token() {
    let temp = tempdir().expect("tempdir");
    let (request, base) = create_runnable_request(&temp);

    let script = write_script(&base, "fail.sh", "exit 42");
    let mut executor = Executor::new(request);

    let token = CancellationToken::new();
    let result = executor.execute_script(&script, &[], &token).await;

    assert!(result.is_err(), "failing script should return error");
    match result.expect_err("must be Err") {
        ExecutorError::ScriptFailed { code, .. } => {
            assert_eq!(code, 42, "exit code should be preserved");
        }
        other => panic!("expected ScriptFailed, got: {other}"),
    }
}

#[test]
fn test_executor_error_cancelled_display() {
    let err = ExecutorError::Cancelled {
        job_id: "my-job-123".to_string(),
    };
    let display = format!("{}", err);
    assert!(display.contains("cancelled"));
    assert!(display.contains("my-job-123"));
}
