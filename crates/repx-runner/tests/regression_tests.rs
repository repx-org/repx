use harness::TestHarness;
use std::fs;
use std::path::Path;

mod harness;

#[test]
fn test_auto_fallback_to_native_when_image_missing_with_bwrap_default() {
    let mut harness = TestHarness::with_execution_type("bwrap");

    let temp_lab_dir = harness.cache_dir.path().join("temp_lab_regression_test");

    let status_cp = std::process::Command::new("cp")
        .arg("-r")
        .arg(&harness.lab_path)
        .arg(&temp_lab_dir)
        .status()
        .expect("Failed to copy lab for regression test");
    assert!(status_cp.success(), "Failed to copy lab directory");

    let status_chmod = std::process::Command::new("chmod")
        .arg("-R")
        .arg("u+w")
        .arg(&temp_lab_dir)
        .status()
        .expect("Failed to chmod lab copy");
    assert!(status_chmod.success(), "Failed to make lab copy writable");

    let find_output = std::process::Command::new("find")
        .arg(&temp_lab_dir)
        .arg("-name")
        .arg("*.json")
        .output()
        .expect("Failed to find JSON files");
    assert!(find_output.status.success());

    let files = String::from_utf8(find_output.stdout).unwrap();
    for file_path_str in files.lines() {
        let path = Path::new(file_path_str);
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = json.as_object_mut() {
                    if obj.remove("image").is_some() {
                        let new_content = serde_json::to_string_pretty(&json).unwrap();
                        fs::write(path, new_content).expect("Failed to write modified JSON");
                    }
                }
            }
        }
    }

    harness.lab_path = temp_lab_dir;

    let mut cmd = harness.cmd();
    cmd.arg("run").arg("simulation-run");

    cmd.assert().success();
}
