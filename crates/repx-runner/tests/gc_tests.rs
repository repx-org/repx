#![allow(dead_code)]

mod harness;
use harness::TestHarness;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;

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

    let job_id = harness.get_job_id_by_name("stage-A-producer");

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
    };
    let config = Config {
        targets: BTreeMap::from([("local".to_string(), target_config)]),
        ..Default::default()
    };

    let client =
        Client::new(config, harness.lab_path.clone()).expect("creating client must succeed");
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

    let job_id = harness.get_job_id_by_name("stage-A-producer");
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
    };
    let config = Config {
        targets: BTreeMap::from([("local".to_string(), target_config)]),
        ..Default::default()
    };

    let client = Client::new(config, lab_path.to_path_buf()).expect("creating client must succeed");
    let target = client
        .get_target("local")
        .expect("getting local target must succeed");
    (client, target)
}

#[test]
fn test_gc_pin_creates_symlink_in_pinned_dir() {
    let harness = TestHarness::new();
    let base_path = &harness.cache_dir;

    let lab_hash = harness.get_lab_content_hash();
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

    let lab_hash = harness.get_lab_content_hash();
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

    let lab_hash = harness.get_lab_content_hash();
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

    let lab_hash = harness.get_lab_content_hash();
    let (_client, target) = make_client_and_target(base_path, &harness.lab_path);

    target
        .pin_gc_root(&lab_hash, "my-pin")
        .expect("pinning gc root must succeed");

    target
        .register_gc_root("test-project", &lab_hash)
        .expect("registering gc root must succeed");

    let roots = target
        .list_gc_roots()
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
        .list_gc_roots()
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

    let lab_hash = harness.get_lab_content_hash();
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

    let lab_hash = harness.get_lab_content_hash();
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
