#![allow(dead_code)]

mod harness;
use harness::TestHarness;
use predicates::prelude::PredicateBooleanExt;
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
        val == "540" || val == "595",
        "Expected 540 or 595, got {}",
        val
    );

    let stage_d_job_id = harness.get_job_id_by_name("stage-D-partial-sums");
    let stage_d_path = harness.get_job_output_path(&stage_d_job_id);
    assert!(stage_d_path.join("repx/SUCCESS").exists());
    assert!(stage_d_path.join("branch-0").exists());
    assert!(stage_d_path.join("branch-9").exists());
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

    let outputs_dir = harness.cache_dir.join("outputs");
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

#[test]
fn test_list_commands() {
    let harness = TestHarness::new();

    harness
        .cmd()
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::contains("Available runs in"))
        .stdout(predicates::str::contains("simulation-run"));

    harness
        .cmd()
        .arg("list")
        .arg("runs")
        .assert()
        .success()
        .stdout(predicates::str::contains("Available runs in"))
        .stdout(predicates::str::contains("simulation-run"));

    harness
        .cmd()
        .arg("list")
        .arg("jobs")
        .arg("simulation-run")
        .assert()
        .success()
        .stdout(predicates::str::contains("Jobs in run 'simulation-run'"))
        .stdout(predicates::str::contains("stage-A-producer"))
        .stdout(predicates::str::contains("stage-B-producer"));

    let job_id = harness.get_job_id_by_name("stage-C-consumer");

    harness
        .cmd()
        .arg("list")
        .arg("deps")
        .arg(&job_id)
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "Dependency tree for job '{}'",
            job_id
        )))
        .stdout(predicates::str::contains(&job_id));

    let full_job_id = harness.get_job_id_by_name("stage-A-producer");
    let partial_job_id = &full_job_id[0..10];

    harness
        .cmd()
        .arg("list")
        .arg("jobs")
        .arg(partial_job_id)
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "Job '{}' found in the following runs:",
            partial_job_id
        )))
        .stdout(predicates::str::contains("simulation-run"))
        .stdout(predicates::str::contains("Jobs in run 'simulation-run'"));

    harness
        .cmd()
        .arg("list")
        .arg("jobs")
        .arg("simulation-run")
        .arg("--stage")
        .arg("stage-A")
        .assert()
        .success()
        .stdout(predicates::str::contains("stage-A-producer"))
        .stdout(predicates::str::is_match("stage-B").unwrap().not());
}

#[test]
fn test_show_commands() {
    let harness = TestHarness::new();

    harness
        .cmd()
        .arg("run")
        .arg("simulation-run")
        .assert()
        .success();

    let job_id = harness.get_job_id_by_name("stage-A-producer");
    let partial_id = &job_id[0..8];

    harness
        .cmd()
        .arg("show")
        .arg("job")
        .arg(partial_id)
        .assert()
        .success()
        .stdout(predicates::str::contains(format!("Job: {}", job_id)))
        .stdout(predicates::str::contains("Status: SUCCESS"))
        .stdout(predicates::str::contains("Paths:"))
        .stdout(predicates::str::contains("output:"));

    harness
        .cmd()
        .arg("show")
        .arg("output")
        .arg(partial_id)
        .assert()
        .success()
        .stdout(predicates::str::contains("Output directory:"));

    harness
        .cmd()
        .arg("show")
        .arg("output")
        .arg(partial_id)
        .arg("numbers.txt")
        .assert()
        .success();

    harness
        .cmd()
        .arg("list")
        .arg("jobs")
        .arg("simulation-run")
        .arg("--stage")
        .arg("stage-A")
        .arg("--output-paths")
        .assert()
        .success()
        .stdout(predicates::str::contains("/out"));
}

#[test]
fn test_continue_on_failure_runs_independent_jobs() {
    let harness = TestHarness::new();

    let stage_a_job_id = harness.get_job_id_by_name("stage-A-producer");
    harness
        .cmd()
        .arg("run")
        .arg(&stage_a_job_id)
        .assert()
        .success();

    let stage_a_path = harness.get_job_output_path(&stage_a_job_id);
    assert!(stage_a_path.join("repx/SUCCESS").exists());

    fs::remove_file(stage_a_path.join("repx/SUCCESS")).unwrap();
    fs::write(stage_a_path.join("repx/FAIL"), "simulated failure").unwrap();

    let stage_b_job_id = harness.get_job_id_by_name("stage-B-producer");
    let stage_b_path = harness.get_job_output_path(&stage_b_job_id);

    let _ = fs::remove_dir_all(&stage_b_path);

    harness
        .cmd()
        .arg("run")
        .arg("--continue-on-failure")
        .arg(&stage_a_job_id)
        .arg(&stage_b_job_id)
        .assert()
        .success();

    assert!(stage_a_path.join("repx/SUCCESS").exists());
    assert!(stage_b_path.join("repx/SUCCESS").exists());
}
