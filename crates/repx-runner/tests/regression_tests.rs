use harness::TestHarness;
use std::fs;

mod harness;

#[test]
fn test_auto_fallback_to_native_when_image_missing_with_bwrap_default() {
    let harness = TestHarness::with_execution_type_and_lab("bwrap", "REFERENCE_LAB_NATIVE_PATH");

    let mut cmd = harness.cmd();
    cmd.arg("run").arg("simulation-run");

    cmd.assert().success();
}

#[test]
fn test_internal_execute_creates_log_file_with_bwrap_command() {
    let harness = TestHarness::with_execution_type("bwrap");
    harness.stage_lab();

    let base_path = &harness.cache_dir;
    let job_id = "job-logging-test";
    harness.stage_job_dirs(job_id);
    let job_out_path = harness.job_output_path(job_id);
    fs::write(job_out_path.join("repx/inputs.json"), "{}")
        .expect("writing inputs.json must succeed");

    let job_package_dir = base_path.join("artifacts/jobs").join(job_id);
    let bin_dir = job_package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("creating job bin dir must succeed");
    let script_path = bin_dir.join("noop.sh");
    fs::write(&script_path, "#!/bin/sh\ntrue\n").expect("writing script must succeed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path)
            .expect("reading script metadata must succeed")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("setting script permissions must succeed");
    }

    let image_tag = harness
        .any_image_tag()
        .expect("No image found in reference lab");

    let cache_temp = tempfile::tempdir().expect("creating temp cache dir must succeed");
    let cache_dir = cache_temp.path().join("repx");
    fs::create_dir_all(&cache_dir).expect("creating repx cache dir must succeed");

    let mut cmd = harness.cmd();
    cmd.env("XDG_CACHE_HOME", cache_temp.path());
    cmd.arg("-vvv")
        .arg("internal-execute")
        .arg("--job-id")
        .arg(job_id)
        .arg("--executable-path")
        .arg(&script_path)
        .arg("--base-path")
        .arg(base_path)
        .arg("--host-tools-dir")
        .arg(harness.host_tools_dir_name())
        .arg("--runtime")
        .arg("bwrap")
        .arg("--image-tag")
        .arg(&image_tag);

    let output = cmd.output().expect("Failed to execute command");
    assert!(
        output.status.success(),
        "internal-execute failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let logs_dir = cache_dir.join("logs");
    assert!(
        logs_dir.exists(),
        "Log directory {:?} must exist after internal-execute",
        logs_dir,
    );

    let internal_logs: Vec<_> = fs::read_dir(&logs_dir)
        .expect("reading logs dir must succeed")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("repx-internal_")
        })
        .collect();

    assert!(
        !internal_logs.is_empty(),
        "Expected at least one repx-internal_*.log file in {:?}, found none. \
         Internal commands must create dedicated log files.",
        logs_dir,
    );

    let log_content = fs::read_to_string(internal_logs[0].path())
        .expect("reading internal log file must succeed");

    assert!(
        log_content.contains("Full bwrap command"),
        "Internal log must contain 'Full bwrap command'. Log content:\n{}",
        log_content,
    );

    assert!(
        log_content.contains("Building bwrap command"),
        "Internal log must contain 'Building bwrap command' with mount_policy info. Log content:\n{}",
        log_content,
    );

    let symlink_path = cache_dir.join("repx-internal.log");
    assert!(
        symlink_path.exists() || symlink_path.symlink_metadata().is_ok(),
        "Symlink {:?} must exist after internal-execute",
        symlink_path,
    );
}
