#![allow(dead_code)]

mod harness;
use harness::TestHarness;
use std::fs;

#[test]
fn test_internal_execute_simple_job_ok() {
    let harness = TestHarness::new();
    let job_id = harness.get_job_id_by_name("stage-A-producer");

    harness.stage_lab();
    harness.stage_job_dirs(&job_id);

    let job_output_path = harness.get_job_output_path(&job_id);
    fs::write(job_output_path.join("repx/inputs.json"), "{}").unwrap();

    let mut cmd = harness.cmd();
    cmd.arg("internal-execute")
        .arg("--job-id")
        .arg(&job_id)
        .arg("--executable-path")
        .arg(harness.get_staged_executable_path(&job_id))
        .arg("--base-path")
        .arg(harness.cache_dir.path())
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("native");

    cmd.assert().success();
    assert!(job_output_path.join("repx/SUCCESS").exists());
    let numbers_out = fs::read_to_string(job_output_path.join("out/numbers.txt")).unwrap();
    assert_eq!(numbers_out.trim(), "1\n2\n3\n4\n5");
}

#[test]
fn test_internal_execute_with_inputs_ok() {
    let harness = TestHarness::new();
    let job_a_id = harness.get_job_id_by_name("stage-A-producer");
    let job_b_id = harness.get_job_id_by_name("stage-B-producer");
    let job_c_id = harness.get_job_id_by_name("stage-C-consumer");

    harness.stage_lab();
    harness.stage_job_dirs(&job_a_id);
    harness.stage_job_dirs(&job_b_id);
    harness.stage_job_dirs(&job_c_id);

    let job_a_out = harness.get_job_output_path(&job_a_id).join("out");
    fs::write(job_a_out.join("numbers.txt"), "1\n2\n").unwrap();
    let job_b_out = harness.get_job_output_path(&job_b_id).join("out");
    fs::write(job_b_out.join("numbers.txt"), "3\n4\n").unwrap();

    let job_c_output_path = harness.get_job_output_path(&job_c_id);
    let inputs_json_content = serde_json::json!({
        "list_a": job_a_out.join("numbers.txt"),
        "list_b": job_b_out.join("numbers.txt")
    });
    fs::write(
        job_c_output_path.join("repx/inputs.json"),
        inputs_json_content.to_string(),
    )
    .unwrap();

    let mut cmd = harness.cmd();
    cmd.arg("internal-execute")
        .arg("--job-id")
        .arg(&job_c_id)
        .arg("--executable-path")
        .arg(harness.get_staged_executable_path(&job_c_id))
        .arg("--base-path")
        .arg(harness.cache_dir.path())
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("native");

    cmd.assert().success();
    assert!(job_c_output_path.join("repx/SUCCESS").exists());
    let combined_list =
        fs::read_to_string(job_c_output_path.join("out/combined_list.txt")).unwrap();
    assert_eq!(combined_list.trim(), "1\n2\n3\n4");
}

#[test]
fn test_internal_execute_fails_if_lab_not_staged() {
    let harness = TestHarness::new();
    let job_id = harness.get_job_id_by_name("stage-A-producer");

    harness.stage_job_dirs(&job_id);
    let job_output_path = harness.get_job_output_path(&job_id);
    fs::write(job_output_path.join("repx/inputs.json"), "{}").unwrap();
    let mut cmd = harness.cmd();
    cmd.arg("internal-execute")
        .arg("--job-id")
        .arg(&job_id)
        .arg("--executable-path")
        .arg(harness.get_staged_executable_path(&job_id))
        .arg("--base-path")
        .arg(harness.cache_dir.path())
        .arg("--host-tools-dir")
        .arg(harness.get_host_tools_dir_name())
        .arg("--runtime")
        .arg("native");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("Execution failed for job"))
        .stderr(predicates::str::contains("No such file or directory"));
}
