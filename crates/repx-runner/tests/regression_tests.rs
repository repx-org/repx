use harness::TestHarness;

mod harness;

#[test]
fn test_auto_fallback_to_native_when_image_missing_with_bwrap_default() {
    let harness = TestHarness::with_execution_type_and_lab("bwrap", "REFERENCE_LAB_NATIVE_PATH");

    let mut cmd = harness.cmd();
    cmd.arg("run").arg("simulation-run");

    cmd.assert().success();
}
