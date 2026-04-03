#![allow(dead_code)]
#![allow(clippy::expect_used)]

mod harness;
use harness::TestHarness;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;

fn create_synthetic_lab(
    artifacts_dir: &Path,
    lab_hash: &str,
    job_ids: &[&str],
) -> std::path::PathBuf {
    let lab_dir = artifacts_dir.join("lab");
    let revision_dir = artifacts_dir.join("revision");
    let jobs_dir = artifacts_dir.join("jobs");
    let host_tools_dir = artifacts_dir
        .join("host-tools")
        .join("fake-tools")
        .join("bin");

    fs::create_dir_all(&lab_dir).expect("create lab dir");
    fs::create_dir_all(&revision_dir).expect("create revision dir");
    fs::create_dir_all(&jobs_dir).expect("create jobs dir");
    fs::create_dir_all(&host_tools_dir).expect("create host-tools dir");

    for id in job_ids {
        fs::create_dir_all(jobs_dir.join(id)).expect("create job dir");
    }

    let jobs_json: serde_json::Value = job_ids
        .iter()
        .map(|id| {
            (
                id.to_string(),
                serde_json::json!({
                    "name": id,
                    "params": {},
                    "stage_type": "simple",
                    "executables": {}
                }),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    let run_metadata = serde_json::json!({
        "name": "test-run",
        "jobs": jobs_json
    });
    let run_path = format!("revision/{}-metadata-test-run.json", lab_hash);
    fs::write(artifacts_dir.join(&run_path), run_metadata.to_string()).expect("write run metadata");

    let root_metadata = serde_json::json!({
        "runs": [run_path],
        "gitHash": "0000000000000000000000000000000000000000",
        "repx_version": env!("CARGO_PKG_VERSION")
    });
    let root_path = format!("revision/{}-metadata-top.json", lab_hash);
    fs::write(artifacts_dir.join(&root_path), root_metadata.to_string())
        .expect("write root metadata");

    let manifest = serde_json::json!({
        "labId": lab_hash,
        "lab_version": env!("CARGO_PKG_VERSION"),
        "metadata": root_path,
        "files": []
    });
    let manifest_path = lab_dir.join(format!("{}-lab-metadata.json", lab_hash));
    fs::write(&manifest_path, manifest.to_string()).expect("write lab manifest");

    manifest_path
}

#[test]
fn test_gc_removes_dead_artifacts_and_outputs() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_dir = base_path.join("gcroots");

    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(&outputs_dir).expect("creating outputs dir must succeed");
    fs::create_dir_all(&gcroots_dir).expect("creating gcroots dir must succeed");

    let dead_artifact = artifacts_dir.join("dead-hash-123");
    fs::create_dir_all(&dead_artifact).expect("creating dead artifact dir must succeed");
    fs::write(dead_artifact.join("some_file"), "data")
        .expect("writing dead artifact file must succeed");

    let dead_output = outputs_dir.join("job-orphan-123");
    fs::create_dir_all(&dead_output).expect("creating dead output dir must succeed");
    fs::write(dead_output.join("stuff.txt"), "result")
        .expect("writing dead output file must succeed");

    let mut cmd = harness.cmd();
    cmd.arg("internal-gc").arg("--base-path").arg(base_path);

    cmd.assert().success();

    assert!(
        !dead_artifact.exists(),
        "Dead artifact should have been deleted"
    );
    assert!(
        !dead_output.exists(),
        "Dead job output should have been deleted"
    );
}

#[test]
fn test_gc_preserves_pinned_lab_and_outputs() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let outputs_dir = base_path.join("outputs");
    let gcroots_pinned = base_path.join("gcroots/pinned");
    let artifacts_dir = base_path.join("artifacts");

    harness.stage_lab();

    let manifest_path = fs::read_dir(artifacts_dir.join("lab"))
        .expect("reading lab artifacts dir must succeed")
        .map(|e| e.expect("reading dir entry must succeed").path())
        .find(|p| p.to_string_lossy().ends_with("lab-metadata.json"))
        .expect("Could not find manifest to pin");

    fs::create_dir_all(&gcroots_pinned).expect("creating gcroots/pinned dir must succeed");
    let link_path = gcroots_pinned.join("my-pinned-lab");
    #[cfg(unix)]
    symlink(&manifest_path, &link_path).expect("Failed to create symlink");

    let job_id = harness.job_id_by_name("stage-A-producer");

    let valid_job_output = outputs_dir.join(&job_id);
    fs::create_dir_all(&valid_job_output).expect("creating valid job output dir must succeed");
    fs::write(valid_job_output.join("log.txt"), "I am important")
        .expect("writing job log must succeed");

    let orphan_job_output = outputs_dir.join("job-nobody-knows");
    fs::create_dir_all(&orphan_job_output).expect("creating orphan job output dir must succeed");

    let mut cmd = harness.cmd();
    cmd.arg("internal-gc").arg("--base-path").arg(base_path);

    let output = cmd.output().expect("executing internal-gc must succeed");
    println!("STDOUT: {}", String::from_utf8_lossy(&output.stdout));
    println!("STDERR: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());

    assert!(manifest_path.exists(), "Manifest file must be preserved");
    assert!(link_path.exists(), "Symlink in gcroots must remain");

    assert!(
        valid_job_output.exists(),
        "Output for job '{}' (present in pinned lab) must be preserved",
        job_id
    );
    assert!(
        !orphan_job_output.exists(),
        "Output for orphan job should be deleted"
    );
}

#[test]
fn test_gc_preserves_auto_gcroots() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let gcroots_auto = base_path.join("gcroots/auto/my-project");
    let artifacts_dir = base_path.join("artifacts");

    harness.stage_lab();

    let manifest_path = fs::read_dir(artifacts_dir.join("lab"))
        .expect("reading lab artifacts dir must succeed")
        .map(|e| e.expect("reading dir entry must succeed").path())
        .find(|p| p.to_string_lossy().ends_with("lab-metadata.json"))
        .expect("Could not find manifest to pin");

    fs::create_dir_all(&gcroots_auto).expect("creating gcroots/auto dir must succeed");
    let link_path = gcroots_auto.join("2023-01-01_snapshot-1");
    #[cfg(unix)]
    symlink(&manifest_path, &link_path).expect("creating auto gcroot symlink must succeed");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        manifest_path.exists(),
        "Artifact referenced by auto-gcroot must be preserved"
    );
}

#[test]
fn test_gc_root_rotation_keeps_last_5() {
    use repx_client::Client;
    use repx_core::config::{Config, Target as TargetConfig};
    use std::collections::BTreeMap;
    use std::thread;
    use std::time::Duration;

    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let project_id = "test-proj-rotation";
    let lab_hash = "abc-123";

    let artifacts_dir = base_path.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(artifacts_dir.join(lab_hash)).expect("creating lab hash dir must succeed");

    let target_config = TargetConfig {
        base_path: base_path.to_path_buf(),
        address: None,
        node_local_path: None,
        default_scheduler: None,
        default_execution_type: None,
        mount_host_paths: false,
        local: None,
        slurm: None,
        mount_paths: vec![],
        artifact_store: None,
    };
    let config = Config {
        targets: BTreeMap::from([("local".to_string(), target_config)]),
        ..Default::default()
    };

    let client =
        Client::new(config, repx_core::lab::LabSource::from_path(&harness.lab_path)).expect("creating client must succeed");
    let target = client
        .get_target("local")
        .expect("getting local target must succeed");

    for _ in 0..7 {
        target
            .register_gc_root(project_id, lab_hash)
            .expect("registering gc root must succeed");
        thread::sleep(Duration::from_millis(1100));
    }

    let gcroots_auto = base_path.join("gcroots/auto").join(project_id);
    let count = fs::read_dir(gcroots_auto)
        .expect("reading gcroots auto dir must succeed")
        .count();
    assert_eq!(count, 5, "Should keep exactly 5 GC roots after rotation");
}

#[test]
fn test_project_id_generation_includes_git_remote() {
    use sha2::{Digest, Sha256};
    use std::process::Command;

    let mut harness = TestHarness::new();
    let temp_lab_root = harness.cache_dir.join("git_test_lab");
    fs::create_dir_all(&temp_lab_root).expect("creating temp lab root must succeed");

    let status = Command::new("cp")
        .arg("-r")
        .arg(format!("{}/.", harness.lab_path.display()))
        .arg(&temp_lab_root)
        .status()
        .expect("Failed to copy lab for git test");
    assert!(status.success());

    harness.lab_path = temp_lab_root.clone();

    let git_init = Command::new("git")
        .arg("init")
        .current_dir(&temp_lab_root)
        .output()
        .expect("Failed to init git");
    assert!(git_init.status.success());

    let remote_url = "https://github.com/test/repx-lab.git";
    let git_remote = Command::new("git")
        .args(["remote", "add", "origin", remote_url])
        .current_dir(&temp_lab_root)
        .output()
        .expect("Failed to add remote");
    assert!(git_remote.status.success());

    harness.stage_lab();

    let lab_abs = fs::canonicalize(&temp_lab_root).expect("canonicalizing lab path must succeed");
    let abs_hash = format!("{:x}", Sha256::digest(lab_abs.to_string_lossy().as_bytes()));
    let remote_hash = format!("{:x}", Sha256::digest(remote_url.as_bytes()));
    let expected_project_id = format!("{}_{}", remote_hash, abs_hash);

    let job_id = harness.job_id_by_name("stage-A-producer");
    harness.cmd().arg("run").arg(job_id).assert().success();

    let gcroots_auto = harness.cache_dir.join("gcroots/auto");
    let project_dir = gcroots_auto.join(&expected_project_id);

    assert!(
        project_dir.exists(),
        "Expected GC root for project ID '{}' not found in {:?}",
        expected_project_id,
        gcroots_auto
    );
}

#[test]
fn test_gc_cleans_collection_directories() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");

    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(base_path.join("gcroots")).expect("creating gcroots dir must succeed");

    let dirs_to_check = vec!["host-tools", "images", "image", "jobs"];

    for dir_name in &dirs_to_check {
        let dir_path = artifacts_dir.join(dir_name);
        fs::create_dir_all(&dir_path).expect("creating collection dir must succeed");
        fs::write(dir_path.join("dead_file"), "content").expect("writing dead file must succeed");
    }

    let bin_dir = artifacts_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("creating bin dir must succeed");
    fs::write(bin_dir.join("keep_me"), "content").expect("writing keep_me file must succeed");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    for dir_name in &dirs_to_check {
        let dir_path = artifacts_dir.join(dir_name);
        if dir_path.exists() {
            assert!(
                !dir_path.join("dead_file").exists(),
                "Content in '{}' should have been deleted",
                dir_name
            );
        }
    }

    assert!(
        bin_dir.join("keep_me").exists(),
        "Bin directory content should be preserved"
    );
}

#[test]
fn test_gc_handles_broken_symlinks_gracefully() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let gcroots_pinned = base_path.join("gcroots/pinned");
    fs::create_dir_all(&gcroots_pinned).expect("creating gcroots/pinned dir must succeed");

    let link_path = gcroots_pinned.join("broken-link");
    #[cfg(unix)]
    symlink(Path::new("/does/not/exist"), &link_path)
        .expect("creating broken symlink must succeed");

    let dead_artifact = base_path.join("artifacts/dead-one");
    fs::create_dir_all(&dead_artifact).expect("creating dead artifact dir must succeed");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        !dead_artifact.exists(),
        "Unreferenced artifact should be deleted despite broken link existence"
    );
}

#[test]
fn test_gc_handles_lab_load_failure() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let gcroots_pinned = base_path.join("gcroots/pinned");

    let corrupt_hash = "corrupt-hash";
    let corrupt_path = base_path.join("artifacts").join(corrupt_hash);
    fs::create_dir_all(&corrupt_path).expect("creating corrupt artifact dir must succeed");

    fs::create_dir_all(&gcroots_pinned).expect("creating gcroots/pinned dir must succeed");
    #[cfg(unix)]
    symlink(&corrupt_path, gcroots_pinned.join("my-corrupt-pin"))
        .expect("creating symlink to corrupt artifact must succeed");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        corrupt_path.exists(),
        "Corrupt artifact pointed to by root must still be preserved"
    );
}

fn make_client_and_target(
    base_path: &std::path::Path,
    lab_path: &std::path::Path,
) -> (
    repx_client::Client,
    std::sync::Arc<dyn repx_client::targets::Target>,
) {
    use repx_client::Client;
    use repx_core::config::{Config, Target as TargetConfig};
    use std::collections::BTreeMap;

    let target_config = TargetConfig {
        base_path: base_path.to_path_buf(),
        address: None,
        node_local_path: None,
        default_scheduler: None,
        default_execution_type: None,
        mount_host_paths: false,
        local: None,
        slurm: None,
        mount_paths: vec![],
        artifact_store: None,
    };
    let config = Config {
        targets: BTreeMap::from([("local".to_string(), target_config)]),
        ..Default::default()
    };

    let client = Client::new(config, repx_core::lab::LabSource::from_path(lab_path)).expect("creating client must succeed");
    let target = client
        .get_target("local")
        .expect("getting local target must succeed");
    (client, target)
}

#[test]
fn test_gc_pin_creates_symlink_in_pinned_dir() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "my-experiment")
        .expect("pinning gc root must succeed");

    let pinned_link = base_path.join("gcroots/pinned/my-experiment");
    assert!(
        pinned_link.symlink_metadata().is_ok(),
        "Pin should create a symlink in gcroots/pinned/"
    );

    let link_target = fs::read_link(&pinned_link).expect("Should be a symlink");
    assert!(
        link_target.to_string_lossy().contains("lab-metadata.json"),
        "Symlink should point to a lab metadata file, got: {:?}",
        link_target
    );
}

#[test]
fn test_gc_pin_default_name_uses_lab_hash() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, &lab_hash)
        .expect("pinning gc root with hash name must succeed");

    let pinned_link = base_path.join("gcroots/pinned").join(&lab_hash);
    assert!(
        pinned_link.symlink_metadata().is_ok(),
        "Pin with lab hash as name should create the symlink"
    );
}

#[test]
fn test_gc_unpin_removes_symlink() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "to-remove")
        .expect("pinning gc root must succeed");

    let pinned_link = base_path.join("gcroots/pinned/to-remove");
    assert!(
        pinned_link.symlink_metadata().is_ok(),
        "Pin should exist before unpin"
    );

    target
        .unpin_gc_root("to-remove")
        .expect("unpinning gc root must succeed");

    assert!(
        pinned_link.symlink_metadata().is_err(),
        "Unpin should remove the symlink"
    );
}

#[test]
fn test_gc_unpin_nonexistent_name_fails() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    let result = target.unpin_gc_root("does-not-exist");
    assert!(result.is_err(), "Unpin of nonexistent name should fail");
    let err_msg = format!(
        "{}",
        result.expect_err("unpinning nonexistent name must fail")
    );
    assert!(
        err_msg.contains("No pinned GC root named"),
        "Error should mention the missing name. Got: {}",
        err_msg
    );
}

#[test]
fn test_gc_pin_nonexistent_hash_fails() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    let result = target.pin_gc_root("nonexistent-hash-xyz", "bad-pin");
    assert!(result.is_err(), "Pin with nonexistent lab hash should fail");
    let err_msg = format!(
        "{}",
        result.expect_err("pinning nonexistent hash must fail")
    );
    assert!(
        err_msg.contains("No lab manifest found"),
        "Error should mention missing manifest. Got: {}",
        err_msg
    );
}

#[test]
fn test_gc_list_shows_auto_and_pinned() {
    use repx_client::targets::GcRootKind;

    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "my-pin")
        .expect("pinning gc root must succeed");

    target
        .register_gc_root("test-project", &lab_hash)
        .expect("registering gc root must succeed");

    let roots = target
        .list_gc_roots(false)
        .expect("listing gc roots must succeed");
    assert!(!roots.is_empty(), "Should have at least 2 roots");

    let has_pinned = roots
        .iter()
        .any(|r| matches!(r.kind, GcRootKind::Pinned) && r.name == "my-pin");
    assert!(has_pinned, "Should contain the pinned root 'my-pin'");

    let has_auto = roots.iter().any(|r| matches!(r.kind, GcRootKind::Auto));
    assert!(has_auto, "Should contain auto roots");
}

#[test]
fn test_gc_list_empty() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    let roots = target
        .list_gc_roots(false)
        .expect("listing gc roots must succeed");
    assert!(roots.is_empty(), "Should have no roots on fresh setup");
}

#[test]
fn test_gc_no_subcommand_still_runs_gc() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let gcroots_dir = base_path.join("gcroots");

    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(&gcroots_dir).expect("creating gcroots dir must succeed");

    let dead_artifact = artifacts_dir.join("dead-hash-999");
    fs::create_dir_all(&dead_artifact).expect("creating dead artifact dir must succeed");
    fs::write(dead_artifact.join("file"), "data").expect("writing dead artifact file must succeed");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        !dead_artifact.exists(),
        "Dead artifact should be collected by internal-gc"
    );
}

#[test]
fn test_pinned_root_survives_gc() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "keep-me")
        .expect("pinning gc root must succeed");

    let dead = base_path.join("artifacts/dead-thing");
    fs::create_dir_all(&dead).expect("creating dead artifact dir must succeed");
    fs::write(dead.join("f"), "data").expect("writing dead artifact file must succeed");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    let pinned_link = base_path.join("gcroots/pinned/keep-me");
    assert!(
        pinned_link.symlink_metadata().is_ok(),
        "Pinned symlink should survive GC"
    );

    let link_target = fs::read_link(&pinned_link).expect("reading pinned symlink must succeed");
    let abs_target = if link_target.is_absolute() {
        link_target
    } else {
        pinned_link
            .parent()
            .expect("pinned link must have parent")
            .join(link_target)
    };
    assert!(
        fs::canonicalize(&abs_target).is_ok(),
        "Pinned root's target artifact should survive GC"
    );

    assert!(!dead.exists(), "Dead artifact should be collected");
}

#[test]
fn test_pin_overwrite_existing() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "same-name")
        .expect("first pin must succeed");
    target
        .pin_gc_root(&lab_hash, "same-name")
        .expect("overwriting pin must succeed");

    let pinned_link = base_path.join("gcroots/pinned/same-name");
    assert!(
        pinned_link.symlink_metadata().is_ok(),
        "Overwritten pin should still exist"
    );

    let count = fs::read_dir(base_path.join("gcroots/pinned"))
        .expect("reading gcroots/pinned dir must succeed")
        .count();
    assert_eq!(
        count, 1,
        "Should have exactly one pinned root after overwrite"
    );
}

#[test]
fn test_gc_dry_run_preserves_all() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_dir = base_path.join("gcroots");

    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(&outputs_dir).expect("creating outputs dir must succeed");
    fs::create_dir_all(&gcroots_dir).expect("creating gcroots dir must succeed");

    let dead_artifact = artifacts_dir.join("dead-hash-dry");
    fs::create_dir_all(&dead_artifact).expect("creating dead artifact dir must succeed");
    fs::write(dead_artifact.join("file.txt"), "data").expect("writing file must succeed");

    let dead_output = outputs_dir.join("job-orphan-dry");
    fs::create_dir_all(&dead_output).expect("creating dead output dir must succeed");
    fs::write(dead_output.join("result.txt"), "result").expect("writing file must succeed");

    let mut cmd = harness.cmd();
    cmd.arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .arg("--dry-run");

    let output = cmd.output().expect("executing internal-gc must succeed");
    assert!(output.status.success());

    assert!(
        dead_artifact.exists(),
        "Dry-run must NOT delete dead artifacts"
    );
    assert!(dead_output.exists(), "Dry-run must NOT delete dead outputs");
}

#[test]
fn test_gc_dry_run_prints_summary() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_dir = base_path.join("gcroots");

    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(&outputs_dir).expect("creating outputs dir must succeed");
    fs::create_dir_all(&gcroots_dir).expect("creating gcroots dir must succeed");

    let dead_artifact = artifacts_dir.join("dead-dry-summary");
    fs::create_dir_all(&dead_artifact).expect("creating dead artifact dir must succeed");
    fs::write(dead_artifact.join("data.bin"), "some data").expect("writing file must succeed");

    let dead_output = outputs_dir.join("job-dry-summary");
    fs::create_dir_all(&dead_output).expect("creating dead output dir must succeed");

    let mut cmd = harness.cmd();
    cmd.arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .arg("--dry-run");

    let output = cmd.output().expect("executing internal-gc must succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(
        stdout.contains("Would delete"),
        "Dry-run stdout should contain 'Would delete'. Got: {}",
        stdout
    );
    assert!(
        stdout.contains("Would free"),
        "Dry-run stdout should contain 'Would free'. Got: {}",
        stdout
    );
}

#[test]
fn test_gc_prints_freed_summary() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_dir = base_path.join("gcroots");

    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(&outputs_dir).expect("creating outputs dir must succeed");
    fs::create_dir_all(&gcroots_dir).expect("creating gcroots dir must succeed");

    let dead_artifact = artifacts_dir.join("dead-freed-summary");
    fs::create_dir_all(&dead_artifact).expect("creating dead artifact dir must succeed");
    fs::write(dead_artifact.join("big_file"), "x".repeat(1024)).expect("writing file must succeed");

    let mut cmd = harness.cmd();
    cmd.arg("internal-gc").arg("--base-path").arg(base_path);

    let output = cmd.output().expect("executing internal-gc must succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(
        stdout.contains("Deleted") && stdout.contains("Freed"),
        "Normal GC stdout should contain 'Deleted' and 'Freed'. Got: {}",
        stdout
    );
}

#[test]
fn test_gc_nothing_to_collect_message() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let gcroots_dir = base_path.join("gcroots");

    fs::create_dir_all(&gcroots_dir).expect("creating gcroots dir must succeed");

    let fresh_base = harness.cache_dir.join("fresh-gc-base");
    fs::create_dir_all(fresh_base.join("gcroots")).expect("creating fresh gcroots must succeed");

    let mut cmd = harness.cmd();
    cmd.arg("internal-gc").arg("--base-path").arg(&fresh_base);

    let output = cmd.output().expect("executing internal-gc must succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(
        stdout.contains("Nothing to collect"),
        "When nothing to GC, should print 'Nothing to collect'. Got: {}",
        stdout
    );
}

#[test]
fn test_gc_remove_auto_roots() {
    use repx_client::Client;
    use repx_core::config::{Config, Target as TargetConfig};
    use std::collections::BTreeMap;
    use std::thread;
    use std::time::Duration;

    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let lab_hash = harness.lab_content_hash();

    let artifacts_dir = base_path.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("creating artifacts dir must succeed");
    fs::create_dir_all(artifacts_dir.join(&lab_hash)).expect("creating lab hash dir must succeed");

    let target_config = TargetConfig {
        base_path: base_path.to_path_buf(),
        address: None,
        node_local_path: None,
        default_scheduler: None,
        default_execution_type: None,
        mount_host_paths: false,
        local: None,
        slurm: None,
        mount_paths: vec![],
        artifact_store: None,
    };
    let config = Config {
        targets: BTreeMap::from([("local".to_string(), target_config)]),
        ..Default::default()
    };

    let client =
        Client::new(config, repx_core::lab::LabSource::from_path(&harness.lab_path)).expect("creating client must succeed");
    let target = client
        .get_target("local")
        .expect("getting local target must succeed");

    for _ in 0..3 {
        target
            .register_gc_root("proj-rm-auto", &lab_hash)
            .expect("registering gc root must succeed");
        thread::sleep(Duration::from_millis(1100));
    }

    target
        .pin_gc_root(&lab_hash, "keep-this")
        .expect("pinning gc root must succeed");

    let before = target
        .list_gc_roots(false)
        .expect("listing gc roots must succeed");
    let auto_before = before
        .iter()
        .filter(|r| matches!(r.kind, repx_client::targets::GcRootKind::Auto))
        .count();
    assert!(auto_before >= 3, "Should have at least 3 auto roots");

    let removed = target
        .remove_auto_roots()
        .expect("removing auto roots must succeed");
    assert!(removed >= 3, "Should have removed at least 3 auto roots");

    let after = target
        .list_gc_roots(false)
        .expect("listing gc roots must succeed");
    let auto_after = after
        .iter()
        .filter(|r| matches!(r.kind, repx_client::targets::GcRootKind::Auto))
        .count();
    assert_eq!(auto_after, 0, "Should have 0 auto roots after remove");

    let pinned_after = after
        .iter()
        .filter(|r| matches!(r.kind, repx_client::targets::GcRootKind::Pinned))
        .count();
    assert_eq!(
        pinned_after, 1,
        "Pinned root should survive remove_auto_roots"
    );
}

#[test]
fn test_gc_list_with_sizes() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "sized-pin")
        .expect("pinning gc root must succeed");

    let roots_no_sizes = target
        .list_gc_roots(false)
        .expect("listing gc roots must succeed");
    assert!(!roots_no_sizes.is_empty());
    for root in &roots_no_sizes {
        assert!(
            root.size_bytes.is_none(),
            "Without --sizes, size_bytes should be None"
        );
    }

    let roots_with_sizes = target
        .list_gc_roots(true)
        .expect("listing gc roots must succeed");
    assert!(!roots_with_sizes.is_empty());
    for root in &roots_with_sizes {
        assert!(
            root.size_bytes.is_some(),
            "With --sizes, size_bytes should be Some"
        );
    }
}

#[test]
fn test_gc_pinned_only_then_collect() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    harness.stage_lab();

    let lab_hash = harness.lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .register_gc_root("proj-pinned-only", &lab_hash)
        .expect("registering gc root must succeed");

    target
        .pin_gc_root(&lab_hash, "pinned-survivor")
        .expect("pinning gc root must succeed");

    let dead = base_path.join("artifacts/dead-pinned-only");
    fs::create_dir_all(&dead).expect("creating dead artifact dir must succeed");
    fs::write(dead.join("f"), "data").expect("writing file must succeed");

    let removed = target
        .remove_auto_roots()
        .expect("removing auto roots must succeed");
    assert!(removed >= 1, "Should have removed auto roots");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(!dead.exists(), "Dead artifact should be collected");

    let pinned_link = base_path.join("gcroots/pinned/pinned-survivor");
    assert!(
        pinned_link.symlink_metadata().is_ok(),
        "Pinned symlink should survive"
    );
}

#[test]
fn test_gc_preserves_live_jobs_and_lab_entries() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_pinned = base_path.join("gcroots/pinned");

    let manifest = create_synthetic_lab(&artifacts_dir, "labhash-A", &["job-alpha", "job-beta"]);

    fs::create_dir_all(&gcroots_pinned).expect("create pinned dir");
    #[cfg(unix)]
    symlink(&manifest, gcroots_pinned.join("pin-A")).expect("symlink pinned root");

    fs::create_dir_all(outputs_dir.join("job-alpha")).expect("create output alpha");
    fs::write(outputs_dir.join("job-alpha/result.txt"), "ok").expect("write alpha output");
    fs::create_dir_all(outputs_dir.join("job-beta")).expect("create output beta");

    fs::create_dir_all(artifacts_dir.join("jobs/job-dead")).expect("create dead job dir");
    fs::create_dir_all(outputs_dir.join("job-dead")).expect("create dead output dir");

    fs::write(
        artifacts_dir.join("lab/deadbeef-lab-metadata.json"),
        "corrupt",
    )
    .expect("write dead manifest");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        artifacts_dir.join("jobs/job-alpha").exists(),
        "Live job-alpha dir in artifacts/jobs/ must be preserved"
    );
    assert!(
        artifacts_dir.join("jobs/job-beta").exists(),
        "Live job-beta dir in artifacts/jobs/ must be preserved"
    );
    assert!(manifest.exists(), "Live lab manifest must be preserved");
    assert!(
        outputs_dir.join("job-alpha/result.txt").exists(),
        "Live job output must be preserved"
    );

    assert!(
        !artifacts_dir.join("jobs/job-dead").exists(),
        "Dead job dir in artifacts/jobs/ must be collected"
    );
    assert!(
        !outputs_dir.join("job-dead").exists(),
        "Dead job output must be collected"
    );
    assert!(
        !artifacts_dir
            .join("lab/deadbeef-lab-metadata.json")
            .exists(),
        "Dead lab manifest must be collected"
    );
}

#[test]
fn test_gc_shared_artifact_survives_via_remaining_root() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_pinned = base_path.join("gcroots/pinned");
    let gcroots_auto = base_path.join("gcroots/auto/test-proj");

    let manifest_a = create_synthetic_lab(
        &artifacts_dir,
        "labhash-shared-A",
        &["job-shared", "job-only-A"],
    );

    let lab_dir = artifacts_dir.join("lab");
    let jobs_dir = artifacts_dir.join("jobs");

    fs::create_dir_all(jobs_dir.join("job-only-B")).expect("create job-only-B dir");

    let run_b = serde_json::json!({
        "name": "test-run-b",
        "jobs": {
            "job-shared": {"name": "job-shared", "params": {}, "stage_type": "simple", "executables": {}},
            "job-only-B": {"name": "job-only-B", "params": {}, "stage_type": "simple", "executables": {}}
        }
    });
    let run_b_path = "revision/labhash-shared-B-metadata-test-run-b.json";
    fs::write(artifacts_dir.join(run_b_path), run_b.to_string()).expect("write run B metadata");

    let root_b = serde_json::json!({
        "runs": [run_b_path],
        "gitHash": "0000000000000000000000000000000000000000",
        "repx_version": env!("CARGO_PKG_VERSION")
    });
    let root_b_path = "revision/labhash-shared-B-metadata-top.json";
    fs::write(artifacts_dir.join(root_b_path), root_b.to_string()).expect("write root B metadata");

    let manifest_b_content = serde_json::json!({
        "labId": "labhash-shared-B",
        "lab_version": env!("CARGO_PKG_VERSION"),
        "metadata": root_b_path,
        "files": []
    });
    let manifest_b = lab_dir.join("labhash-shared-B-lab-metadata.json");
    fs::write(&manifest_b, manifest_b_content.to_string()).expect("write manifest B");

    fs::create_dir_all(&gcroots_pinned).expect("create pinned dir");
    fs::create_dir_all(&gcroots_auto).expect("create auto dir");
    #[cfg(unix)]
    {
        symlink(&manifest_a, gcroots_pinned.join("pin-A")).expect("pin A");
        symlink(&manifest_b, gcroots_auto.join("auto-B")).expect("auto B");
    }

    for job in &["job-shared", "job-only-A", "job-only-B"] {
        fs::create_dir_all(outputs_dir.join(job)).expect("create output");
        fs::write(outputs_dir.join(job).join("data.txt"), "result").expect("write output");
    }

    assert!(gcroots_pinned.join("pin-A").symlink_metadata().is_ok());
    assert!(gcroots_auto.join("auto-B").symlink_metadata().is_ok());

    fs::remove_file(gcroots_auto.join("auto-B")).expect("remove auto-B root");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        artifacts_dir.join("jobs/job-shared").exists(),
        "Shared job dir must survive via the remaining pinned root"
    );
    assert!(
        outputs_dir.join("job-shared/data.txt").exists(),
        "Shared job output must survive via the remaining pinned root"
    );

    assert!(
        artifacts_dir.join("jobs/job-only-A").exists(),
        "job-only-A must survive (lab A is still pinned)"
    );
    assert!(
        outputs_dir.join("job-only-A/data.txt").exists(),
        "job-only-A output must survive"
    );

    assert!(
        !artifacts_dir.join("jobs/job-only-B").exists(),
        "job-only-B artifact must be collected (lab B root was removed)"
    );
    assert!(
        !outputs_dir.join("job-only-B").exists(),
        "job-only-B output must be collected"
    );

    assert!(
        manifest_a.exists(),
        "Lab A manifest must survive (still pinned)"
    );
    assert!(
        !manifest_b.exists(),
        "Lab B manifest must be collected (no root)"
    );
}

#[test]
fn test_gc_incomplete_lab_does_not_block_other_labs() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let outputs_dir = base_path.join("outputs");
    let gcroots_pinned = base_path.join("gcroots/pinned");

    let manifest_good = create_synthetic_lab(
        &artifacts_dir,
        "labhash-good",
        &["job-good-1", "job-good-2"],
    );

    let lab_dir = artifacts_dir.join("lab");
    let jobs_dir = artifacts_dir.join("jobs");

    fs::create_dir_all(jobs_dir.join("job-incomplete-1")).expect("create incomplete job dir");

    let run_inc = serde_json::json!({
        "name": "test-run-inc",
        "jobs": {
            "job-incomplete-1": {
                "name": "job-incomplete-1",
                "params": {},
                "stage_type": "simple",
                "executables": {}
            }
        }
    });
    let run_inc_path = "revision/labhash-incomplete-metadata-test-run-inc.json";
    fs::write(artifacts_dir.join(run_inc_path), run_inc.to_string())
        .expect("write incomplete run metadata");

    let root_inc = serde_json::json!({
        "runs": [run_inc_path],
        "gitHash": "0000000000000000000000000000000000000000",
        "repx_version": env!("CARGO_PKG_VERSION")
    });
    let root_inc_path = "revision/labhash-incomplete-metadata-top.json";
    fs::write(artifacts_dir.join(root_inc_path), root_inc.to_string())
        .expect("write incomplete root metadata");

    let manifest_inc_content = serde_json::json!({
        "labId": "labhash-incomplete",
        "lab_version": env!("CARGO_PKG_VERSION"),
        "metadata": root_inc_path,
        "files": [
            {
                "path": "store/nonexistent-nix-hash-binary",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }
        ]
    });
    let manifest_incomplete = lab_dir.join("labhash-incomplete-lab-metadata.json");
    fs::write(&manifest_incomplete, manifest_inc_content.to_string())
        .expect("write incomplete manifest");

    fs::create_dir_all(&gcroots_pinned).expect("create pinned dir");
    #[cfg(unix)]
    {
        symlink(&manifest_good, gcroots_pinned.join("pin-good")).expect("pin good");
        symlink(&manifest_incomplete, gcroots_pinned.join("pin-incomplete"))
            .expect("pin incomplete");
    }

    for job in &["job-good-1", "job-good-2", "job-incomplete-1"] {
        fs::create_dir_all(outputs_dir.join(job)).expect("create output");
        fs::write(outputs_dir.join(job).join("out.txt"), "data").expect("write output");
    }

    fs::create_dir_all(artifacts_dir.join("jobs/job-dead-orphan")).expect("create dead job");
    fs::create_dir_all(outputs_dir.join("job-dead-orphan")).expect("create dead output");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    assert!(
        artifacts_dir.join("jobs/job-good-1").exists(),
        "job-good-1 artifact must survive"
    );
    assert!(
        artifacts_dir.join("jobs/job-good-2").exists(),
        "job-good-2 artifact must survive"
    );
    assert!(
        outputs_dir.join("job-good-1/out.txt").exists(),
        "job-good-1 output must survive"
    );
    assert!(
        outputs_dir.join("job-good-2/out.txt").exists(),
        "job-good-2 output must survive"
    );

    assert!(
        artifacts_dir.join("jobs/job-incomplete-1").exists(),
        "job-incomplete-1 artifact must survive (unchecked load should succeed)"
    );
    assert!(
        outputs_dir.join("job-incomplete-1/out.txt").exists(),
        "job-incomplete-1 output must survive"
    );

    assert!(
        !artifacts_dir.join("jobs/job-dead-orphan").exists(),
        "Dead orphan job artifact must be collected"
    );
    assert!(
        !outputs_dir.join("job-dead-orphan").exists(),
        "Dead orphan job output must be collected"
    );
}

#[test]
fn test_gc_preserves_store_entries_for_live_labs() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;
    let artifacts_dir = base_path.join("artifacts");
    let gcroots_pinned = base_path.join("gcroots/pinned");

    let lab_hash = "labhash-store-test";
    let live_store_entries = ["abc123-jq-static-bin-jq", "def456-bubblewrap-static-bwrap"];
    let dead_store_entry = "zzz999-orphan-tool-nobody-wants";

    let lab_dir = artifacts_dir.join("lab");
    let revision_dir = artifacts_dir.join("revision");
    let jobs_dir = artifacts_dir.join("jobs");
    let store_dir = artifacts_dir.join("store");
    let host_tools_dir = artifacts_dir
        .join("host-tools")
        .join("fake-tools")
        .join("bin");

    fs::create_dir_all(&lab_dir).expect("create lab dir");
    fs::create_dir_all(&revision_dir).expect("create revision dir");
    fs::create_dir_all(&jobs_dir).expect("create jobs dir");
    fs::create_dir_all(&store_dir).expect("create store dir");
    fs::create_dir_all(&host_tools_dir).expect("create host-tools dir");

    fs::create_dir_all(jobs_dir.join("job-store-user")).expect("create job dir");

    for entry in &live_store_entries {
        fs::write(store_dir.join(entry), "binary-content").expect("write store entry");
    }

    fs::write(store_dir.join(dead_store_entry), "dead-content").expect("write dead store entry");

    let files: Vec<serde_json::Value> = live_store_entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "path": format!("store/{}", e),
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            })
        })
        .collect();

    let run = serde_json::json!({
        "name": "test-run",
        "jobs": {
            "job-store-user": {
                "name": "job-store-user",
                "params": {},
                "stage_type": "simple",
                "executables": {}
            }
        }
    });
    let run_path = format!("revision/{}-metadata-test-run.json", lab_hash);
    fs::write(artifacts_dir.join(&run_path), run.to_string()).expect("write run metadata");

    let root = serde_json::json!({
        "runs": [&run_path],
        "gitHash": "0000000000000000000000000000000000000000",
        "repx_version": env!("CARGO_PKG_VERSION")
    });
    let root_path = format!("revision/{}-metadata-top.json", lab_hash);
    fs::write(artifacts_dir.join(&root_path), root.to_string()).expect("write root metadata");

    let manifest_content = serde_json::json!({
        "labId": lab_hash,
        "lab_version": env!("CARGO_PKG_VERSION"),
        "metadata": &root_path,
        "files": files
    });
    let manifest_path = lab_dir.join(format!("{}-lab-metadata.json", lab_hash));
    fs::write(&manifest_path, manifest_content.to_string()).expect("write manifest");

    fs::create_dir_all(&gcroots_pinned).expect("create pinned dir");
    #[cfg(unix)]
    symlink(&manifest_path, gcroots_pinned.join("pin-store-test")).expect("pin lab");

    harness
        .cmd()
        .arg("internal-gc")
        .arg("--base-path")
        .arg(base_path)
        .assert()
        .success();

    for entry in &live_store_entries {
        assert!(
            store_dir.join(entry).exists(),
            "Live store entry '{}' referenced by pinned lab must survive GC",
            entry
        );
    }

    assert!(
        !store_dir.join(dead_store_entry).exists(),
        "Dead store entry '{}' must be collected by GC",
        dead_store_entry
    );
}
