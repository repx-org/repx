#![allow(dead_code)]

mod harness;
use harness::TestHarness;
use std::fs;
#[test]
fn test_full_run_local_bwrap() {
    let harness = TestHarness::with_execution_type("bwrap");
    let artifacts_dir = harness.cache_dir.path().join("artifacts");
    harness.stage_lab();

    let host_tools_bin = artifacts_dir.join("host-tools").join("bin");

    eprintln!(
        "\n[TEST DEBUG] Inspecting Host Tools Directory: {:?}",
        host_tools_bin
    );
    if host_tools_bin.exists() {
        let mut entries: Vec<_> = fs::read_dir(&host_tools_bin)
            .unwrap()
            .map(|r| r.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();
        eprintln!("[TEST DEBUG] Files found: {:?}", entries);

        if entries.contains(&"tar".to_string()) {
            eprintln!(
                "[TEST DEBUG] 🚨 CRITICAL: 'tar' WAS FOUND IN THE TEST HARNESS DIRECTORY! 🚨"
            );
        } else {
            eprintln!("[TEST DEBUG] ✅ 'tar' is NOT in the directory.");
        }
    } else {
        eprintln!("[TEST DEBUG] Directory does not exist (yet)!");
    }

    let mut cmd = harness.cmd();
    cmd.arg("run").arg("simulation-run");

    cmd.assert().success();

    let stage_e_job_id = harness.get_job_id_by_name("stage-E-total-sum");
    let stage_e_path = harness.get_job_output_path(&stage_e_job_id);

    assert!(stage_e_path.join("repx/SUCCESS").exists());
    let total_sum_content = fs::read_to_string(stage_e_path.join("out/total_sum.txt")).unwrap();
    let val = total_sum_content.trim();
    assert!(
        val == "400" || val == "415",
        "Expected 400 or 415, got {}",
        val
    );
}
#[test]
fn test_bwrap_isolation_properties() {
    let harness = TestHarness::with_execution_type("bwrap");
    harness.stage_lab();

    let base_path = harness.cache_dir.path();
    let victim_dir = base_path.join("outputs").join("victim").join("out");
    fs::create_dir_all(&victim_dir).unwrap();
    let victim_file = victim_dir.join("secret.txt");
    fs::write(&victim_file, "original content").unwrap();

    let artifact_file = base_path.join("artifacts").join("host_file.txt");
    fs::write(&artifact_file, "host content").unwrap();

    let attacker_job_id = "job-attacker";
    harness.stage_job_dirs(attacker_job_id);
    let attacker_out_path = harness.get_job_output_path(attacker_job_id);
    fs::write(attacker_out_path.join("repx/inputs.json"), "{}").unwrap();

    let job_package_dir = base_path.join("artifacts/jobs").join(attacker_job_id);
    let bin_dir = job_package_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script_path = bin_dir.join("attack.sh");

    let script_content = format!(
        r#"#!/bin/sh
echo "pwned" > "{}" 2>/dev/null
if [ $? -eq 0 ]; then
  echo "FAIL: Managed to overwrite victim file"
  exit 1
fi

echo "pwned" > "{}" 2>/dev/null
if [ $? -eq 0 ]; then
  echo "FAIL: Managed to overwrite artifact file"
  exit 1
fi

echo "success" > "own_output.txt"
if [ $? -ne 0 ]; then
  echo "FAIL: Could not write to own output"
  exit 1
fi

echo "SUCCESS: Isolation verified"
exit 0
"#,
        victim_file.to_string_lossy(),
        artifact_file.to_string_lossy()
    );

    fs::write(&script_path, script_content).unwrap();

    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let image_tag = harness
        .get_any_image_tag()
        .expect("No image found in lab metadata");

    let mut cmd = harness.cmd();
    cmd.arg("internal-execute")
        .arg("--job-id")
        .arg(attacker_job_id)
        .arg("--executable-path")
        .arg(&script_path)
        .arg("--base-path")
        .arg(base_path)
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("bwrap")
        .arg("--image-tag")
        .arg(image_tag);

    let output = cmd.output().expect("Failed to execute command");

    println!("STDOUT: {}", String::from_utf8_lossy(&output.stdout));
    println!("STDERR: {}", String::from_utf8_lossy(&output.stderr));
    let repx_err =
        fs::read_to_string(attacker_out_path.join("repx/stderr.log")).unwrap_or_default();
    println!("REPX STDERR: {}", repx_err);

    let victim_content = fs::read_to_string(&victim_file).expect("Failed to read victim file");
    assert_eq!(
        victim_content, "original content",
        "Victim file was modified! Isolation failed."
    );

    let artifact_content =
        fs::read_to_string(&artifact_file).expect("Failed to read artifact file");
    assert_eq!(
        artifact_content, "host content",
        "Artifact file was modified! Isolation failed."
    );

    let own_out = attacker_out_path.join("out/own_output.txt");
    assert!(
        own_out.exists(),
        "Own output file not found. Script might have failed early."
    );

    assert!(
        output.status.success(),
        "Attacker script failed (check stdout for FAIL messages)"
    );
}

#[test]
fn test_bwrap_impure_mode_access_host() {
    let harness = TestHarness::with_execution_type("bwrap");
    harness.stage_lab();

    let base_path = harness.cache_dir.path();
    let host_marker_file = base_path.join("i_am_on_host.txt");
    fs::write(&host_marker_file, "host data").unwrap();

    let job_id = "job-impure";
    harness.stage_job_dirs(job_id);
    let job_out_path = harness.get_job_output_path(job_id);
    fs::write(job_out_path.join("repx/inputs.json"), "{}").unwrap();

    let job_package_dir = base_path.join("artifacts/jobs").join(job_id);
    let bin_dir = job_package_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script_path = bin_dir.join("check_host.sh");

    let temp_outside = tempfile::tempdir().unwrap();
    let outside_file = temp_outside.path().join("outside.txt");
    fs::write(&outside_file, "outside").unwrap();

    let shell = if std::path::Path::new("/bin/sh").exists() {
        "/bin/sh".to_string()
    } else if std::path::Path::new("/usr/bin/env").exists() {
        "/usr/bin/env sh".to_string()
    } else {
        std::env::var("SHELL").unwrap_or("/bin/sh".to_string())
    };

    let script_content = format!(
        r#"#!{}
if [ -f "{}" ]; then
    echo "FOUND" > found.txt
else
    echo "NOT_FOUND" > found.txt
fi
"#,
        shell,
        outside_file.to_string_lossy()
    );

    fs::write(&script_path, script_content).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

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
        .arg("bwrap")
        .arg("--image-tag")
        .arg(&image_tag);

    cmd.assert().success();
    let content = fs::read_to_string(job_out_path.join("out/found.txt")).unwrap();
    assert_eq!(
        content.trim(),
        "NOT_FOUND",
        "Pure mode should NOT see host temp files"
    );

    fs::remove_file(job_out_path.join("out/found.txt")).unwrap();
    fs::remove_file(job_out_path.join("repx/SUCCESS")).unwrap();

    let mut cmd2 = harness.cmd();
    cmd2.arg("internal-execute")
        .arg("--job-id")
        .arg(job_id)
        .arg("--executable-path")
        .arg(&script_path)
        .arg("--base-path")
        .arg(base_path)
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("bwrap")
        .arg("--image-tag")
        .arg(&image_tag)
        .arg("--mount-host-paths");

    cmd2.assert().success();
    let content2 = fs::read_to_string(job_out_path.join("out/found.txt")).unwrap();
    assert_eq!(
        content2.trim(),
        "FOUND",
        "Impure mode SHOULD see host temp files"
    );
}

#[test]
fn test_bwrap_mount_paths_specific() {
    let harness = TestHarness::with_execution_type("bwrap");
    harness.stage_lab();

    let base_path = harness.cache_dir.path();
    let job_id = "job-mount-paths";
    harness.stage_job_dirs(job_id);
    let job_out_path = harness.get_job_output_path(job_id);
    fs::write(job_out_path.join("repx/inputs.json"), "{}").unwrap();

    let job_package_dir = base_path.join("artifacts/jobs").join(job_id);
    let bin_dir = job_package_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script_path = bin_dir.join("check_specific.sh");

    let temp_outside = tempfile::tempdir().unwrap();
    let secret_file = temp_outside.path().join("secret.txt");
    fs::write(&secret_file, "secret-data").unwrap();

    let shell = if std::path::Path::new("/bin/sh").exists() {
        "/bin/sh".to_string()
    } else if std::path::Path::new("/usr/bin/env").exists() {
        "/usr/bin/env sh".to_string()
    } else {
        std::env::var("SHELL").unwrap_or("/bin/sh".to_string())
    };

    let script_content = format!(
        r#"#!{}
if [ -f "{}" ]; then
    echo "FOUND" > found.txt
else
    echo "NOT_FOUND" > found.txt
fi
"#,
        shell,
        secret_file.to_string_lossy()
    );

    fs::write(&script_path, script_content).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

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
        .arg("bwrap")
        .arg("--image-tag")
        .arg(&image_tag);

    cmd.assert().success();
    let content = fs::read_to_string(job_out_path.join("out/found.txt")).unwrap();
    assert_eq!(
        content.trim(),
        "NOT_FOUND",
        "Without mount-paths, should NOT see external file"
    );

    fs::remove_file(job_out_path.join("out/found.txt")).unwrap();
    fs::remove_file(job_out_path.join("repx/SUCCESS")).unwrap();

    let mut cmd2 = harness.cmd();
    cmd2.arg("internal-execute")
        .arg("--job-id")
        .arg(job_id)
        .arg("--executable-path")
        .arg(&script_path)
        .arg("--base-path")
        .arg(base_path)
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("bwrap")
        .arg("--image-tag")
        .arg(&image_tag)
        .arg("--mount-paths")
        .arg(secret_file.to_string_lossy().to_string());

    cmd2.assert().success();
    let content2 = fs::read_to_string(job_out_path.join("out/found.txt")).unwrap();
    assert_eq!(
        content2.trim(),
        "FOUND",
        "With --mount-paths, SHOULD see external file"
    );
}
