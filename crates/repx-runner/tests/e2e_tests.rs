#![allow(dead_code)]

mod harness;
use harness::TestHarness;
use std::fs;

#[test]
fn test_full_run_local_native() {
    let harness = TestHarness::new();

    let mut cmd = harness.cmd();
    cmd.arg("run").arg("simulation-run");

    cmd.assert().success();

    let stage_e_job_id = harness.get_job_id_by_name("stage-E-total-sum");
    let stage_e_path = harness.get_job_output_path(&stage_e_job_id);
    assert!(stage_e_path.join("repx/SUCCESS").exists());
    let total_sum_content = fs::read_to_string(stage_e_path.join("out/total_sum.txt")).unwrap();
    assert_eq!(total_sum_content.trim(), "385");

    let stage_d_job_id = harness.get_job_id_by_name("stage-D-partial-sums");
    let stage_d_path = harness.get_job_output_path(&stage_d_job_id);
    assert!(stage_d_path.join("repx/SUCCESS").exists());
    assert!(stage_d_path.join("worker-0").exists());
    assert!(stage_d_path.join("worker-9").exists());
}

#[test]
fn test_idempotent_run_local_native() {
    let harness = TestHarness::new();

    harness
        .cmd()
        .arg("run")
        .arg("simulation-run")
        .assert()
        .success();

    let mut cmd2 = harness.cmd();
    cmd2.arg("run").arg("simulation-run");

    cmd2.assert().success().stdout(predicates::str::contains(
        "All required jobs for this submission are already complete.",
    ));
}

#[test]
fn test_partial_run_by_job_id() {
    let harness = TestHarness::new();

    let stage_c_job_id = harness.get_job_id_by_name("stage-C-consumer");
    let mut cmd = harness.cmd();
    cmd.arg("run").arg(&stage_c_job_id);

    cmd.assert().success();

    let outputs_dir = harness.cache_dir.path().join("outputs");

    for name in ["stage-A-producer", "stage-B-producer", "stage-C-consumer"] {
        let job_id = harness.get_job_id_by_name(name);
        let stage_path = outputs_dir.join(job_id);
        assert!(stage_path.join("repx/SUCCESS").exists());
    }

    let stage_d_job_id = harness.get_job_id_by_name("stage-D-partial-sums");
    let stage_e_job_id = harness.get_job_id_by_name("stage-E-total-sum");

    assert!(
        !outputs_dir.join(stage_d_job_id).exists(),
        "Stage D ran but should not have"
    );
    assert!(
        !outputs_dir.join(stage_e_job_id).exists(),
        "Stage E ran but should not have"
    );
}
