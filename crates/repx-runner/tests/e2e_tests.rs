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
    let val = total_sum_content.trim();
    assert!(
        val == "400" || val == "415",
        "Expected 400 or 415, got {}",
        val
    );

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

    let c_job_data = &harness.metadata["jobs"][&stage_c_job_id];
    let inputs = c_job_data["executables"]["main"]["inputs"]
        .as_array()
        .expect("Could not find inputs for stage C job");

    let dependency_job_ids: Vec<String> = inputs
        .iter()
        .map(|mapping| {
            mapping["job_id"]
                .as_str()
                .expect("job_id not a string")
                .to_string()
        })
        .collect();
    assert_eq!(
        dependency_job_ids.len(),
        2,
        "Stage C should have exactly 2 dependencies"
    );

    let mut cmd = harness.cmd();
    cmd.arg("run").arg(&stage_c_job_id);
    cmd.assert().success();

    let outputs_dir = harness.cache_dir.path().join("outputs");
    let mut jobs_that_should_have_run = dependency_job_ids;
    jobs_that_should_have_run.push(stage_c_job_id.clone());

    for job_id in &jobs_that_should_have_run {
        let stage_path = outputs_dir.join(job_id);
        assert!(
            stage_path.join("repx/SUCCESS").exists(),
            "Job {} was expected to succeed but did not",
            stage_path.display()
        );
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
