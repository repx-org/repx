use crate::harness::TestHarness;
use std::fs;
use std::os::unix::fs::PermissionsExt;

mod harness;

#[test]
fn test_podman_impure_mode_args() {
    let harness = TestHarness::with_execution_type("podman");
    harness.stage_lab();

    let base_path = &harness.cache_dir;
    let job_id = "job-podman-impure";
    harness.stage_job_dirs(job_id);

    let host_tools_dir = harness
        .cache_dir
        .join("artifacts/host-tools")
        .join(harness.get_host_tools_dir_name())
        .join("bin");

    fs::create_dir_all(&host_tools_dir).expect("creating host-tools dir must succeed");
    let mock_podman_path = host_tools_dir.join("podman");

    if mock_podman_path.exists() {
        fs::remove_file(&mock_podman_path).expect("removing existing mock podman must succeed");
    }

    let log_file = base_path.join("podman_args.log");

    let mock_content = format!(
        r#"#!/bin/sh
echo "$@" > "{}"
# Mock success by creating expected output files if needed
# For internal-execute, we just need to exit successfully
exit 0
"#,
        log_file.display()
    );

    fs::write(&mock_podman_path, mock_content).expect("writing mock podman script must succeed");
    let mut perms = fs::metadata(&mock_podman_path)
        .expect("reading mock podman metadata must succeed")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_podman_path, perms)
        .expect("setting mock podman permissions must succeed");

    let job_out_path = harness.get_job_output_path(job_id);
    fs::write(job_out_path.join("repx/inputs.json"), "{}")
        .expect("writing inputs.json must succeed");

    let job_package_dir = base_path.join("artifacts/jobs").join(job_id);
    let bin_dir = job_package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("creating job bin dir must succeed");
    let script_path = bin_dir.join("script.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0").expect("writing script.sh must succeed");

    let mut perms_script = fs::metadata(&script_path)
        .expect("reading script metadata must succeed")
        .permissions();
    perms_script.set_mode(0o755);
    fs::set_permissions(&script_path, perms_script)
        .expect("setting script permissions must succeed");

    let image_tag = harness.get_any_image_tag().expect("No image found");

    let mut cmd = harness.cmd();
    cmd.arg("internal-execute")
        .arg("--job-id")
        .arg(job_id)
        .arg("--executable-path")
        .arg(&script_path)
        .arg("--base-path")
        .arg(base_path)
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("podman")
        .arg("--image-tag")
        .arg(&image_tag)
        .arg("--mount-host-paths");

    cmd.assert().success();

    let args = fs::read_to_string(&log_file).expect("reading podman args log must succeed");

    assert!(
        args.contains("-v /home:/home"),
        "Missing /home mount in impure mode"
    );
    assert!(
        args.contains("-v /tmp:/tmp"),
        "Missing /tmp mount in impure mode"
    );

    if std::path::Path::new("/nix").exists() {
        assert!(
            args.contains("-v /nix:/nix"),
            "Missing /nix mount when /nix exists"
        );
    }
}

#[test]
fn test_podman_mount_specific_paths_args() {
    let harness = TestHarness::with_execution_type("podman");
    harness.stage_lab();

    let base_path = &harness.cache_dir;
    let job_id = "job-podman-specific";
    harness.stage_job_dirs(job_id);

    let host_tools_dir = harness
        .cache_dir
        .join("artifacts/host-tools")
        .join(harness.get_host_tools_dir_name())
        .join("bin");

    fs::create_dir_all(&host_tools_dir).expect("creating host-tools dir must succeed");
    let mock_podman_path = host_tools_dir.join("podman");

    if mock_podman_path.exists() {
        fs::remove_file(&mock_podman_path).expect("removing existing mock podman must succeed");
    }

    let log_file = base_path.join("podman_specific_args.log");

    let mock_content = format!(
        r#"#!/bin/sh
echo "$@" > "{}"
exit 0
"#,
        log_file.display()
    );

    fs::write(&mock_podman_path, mock_content).expect("writing mock podman script must succeed");
    let mut perms = fs::metadata(&mock_podman_path)
        .expect("reading mock podman metadata must succeed")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&mock_podman_path, perms)
        .expect("setting mock podman permissions must succeed");

    let job_out_path = harness.get_job_output_path(job_id);
    fs::write(job_out_path.join("repx/inputs.json"), "{}")
        .expect("writing inputs.json must succeed");
    let job_package_dir = base_path.join("artifacts/jobs").join(job_id);
    let bin_dir = job_package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("creating job bin dir must succeed");
    let script_path = bin_dir.join("script.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0").expect("writing script.sh must succeed");
    let mut perms_script = fs::metadata(&script_path)
        .expect("reading script metadata must succeed")
        .permissions();
    perms_script.set_mode(0o755);
    fs::set_permissions(&script_path, perms_script)
        .expect("setting script permissions must succeed");

    let image_tag = harness.get_any_image_tag().expect("No image found");

    let path1 = "/tmp/my-secret-1";
    let path2 = "/opt/tools/custom";

    let mut cmd = harness.cmd();
    cmd.arg("internal-execute")
        .arg("--job-id")
        .arg(job_id)
        .arg("--executable-path")
        .arg(&script_path)
        .arg("--base-path")
        .arg(base_path)
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("podman")
        .arg("--image-tag")
        .arg(&image_tag)
        .arg("--mount-paths")
        .arg(path1)
        .arg("--mount-paths")
        .arg(path2);

    cmd.assert().success();

    let args =
        fs::read_to_string(&log_file).expect("reading podman specific args log must succeed");

    assert!(
        args.contains(&format!("-v {}:{}", path1, path1)),
        "Missing first specific mount"
    );
    assert!(
        args.contains(&format!("-v {}:{}", path2, path2)),
        "Missing second specific mount"
    );

    assert!(
        !args.contains("-v /home:/home"),
        "Should NOT mount /home in specific mode"
    );
}
