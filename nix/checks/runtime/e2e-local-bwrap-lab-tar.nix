{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
in

pkgs.testers.runNixOSTest {
  name = "e2e-local-bwrap-lab-tar";

  extraPythonPackages = _: [ getSubsetJobs ];

  nodes.machine = _: {
    virtualisation = {
      diskSize = 25600;
      memorySize = 4096;
      cores = 2;
    };
    environment.systemPackages = [
      repx
      pkgs.bubblewrap
      pkgs.jq
    ];
  };

  testScript = ''
    from get_subset_jobs import get_subset_jobs
    start_all()

    base_path = "/mnt/shared/repx-store"
    node_local = "/local/scratch"

    machine.succeed(f"mkdir -p {base_path}")
    machine.succeed(f"mkdir -p {node_local}")

    machine.succeed("mkdir -p /root/.config/repx")

    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    node_local_path = "{node_local}"
    default_scheduler = "local"
    default_execution_type = "bwrap"
    [targets.local.local]
    execution_types = ["bwrap"]
    local_concurrency = 4
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed(f"mkdir -p {base_path}/artifacts/host-tools/default/bin")
    machine.succeed(f"ln -s $(which bwrap) {base_path}/artifacts/host-tools/default/bin/bwrap")

    subset_jobs = get_subset_jobs("${referenceLab}")
    if not subset_jobs:
        raise Exception("get_subset_jobs returned empty list for e2e-local-bwrap-lab-tar")
    run_args = " ".join(subset_jobs)
    print(f"Running subset: {run_args}")

    with subtest("Jobs succeed with --lab-tar"):
        print("--- Testing bwrap with --lab-tar ---")
        machine.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")

        success_count = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
        print(f"Found {success_count} SUCCESS markers")

        if success_count == 0:
            raise Exception("No SUCCESS markers found!")

    with subtest("Outputs land on shared storage, not node-local"):
        output_count = int(machine.succeed(f"find {base_path}/outputs -type f | wc -l").strip())
        local_output = machine.succeed(f"find {node_local} -path '*/outputs/*' 2>/dev/null | wc -l").strip()
        print(f"Files in base_path/outputs: {output_count}")
        print(f"Files in node_local/outputs: {local_output}")

        if output_count == 0:
            raise Exception("No output files found on shared storage!")
        if int(local_output) > 0:
            raise Exception("Output files leaked to node-local storage!")

    with subtest("Lab tar extracted to node-local"):
        lab_dirs = machine.succeed(f"find {node_local}/repx/labs -maxdepth 1 -mindepth 1 -type d 2>/dev/null").strip()
        print(f"Lab dirs on node-local: {lab_dirs}")
        if not lab_dirs:
            raise Exception("No lab extraction directory found on node-local!")

        marker_count = int(machine.succeed(
            f"find {node_local}/repx/labs -maxdepth 1 -name '.*.repx-cache.json' 2>/dev/null | wc -l"
        ).strip())
        print(f"Extraction cache markers: {marker_count}")
        if marker_count == 0:
            raise Exception("No extraction cache marker found!")

    with subtest("Image cache on node-local via lab-tar extraction"):
        local_cache = int(machine.succeed(
            f"find {node_local}/repx/labs -path '*/cache/images/*/SUCCESS' 2>/dev/null | wc -l"
        ).strip())
        print(f"Image cache SUCCESS markers on node-local: {local_cache}")
        if local_cache == 0:
            raise Exception("No image cache found on node-local storage!")

    with subtest("Cache is reused across jobs (single extraction)"):
        marker_count = int(machine.succeed(
            f"find {node_local}/repx/labs -maxdepth 1 -name '.*.repx-cache.json' | wc -l"
        ).strip())
        print(f"Extraction cache markers: {marker_count}")
        if marker_count > 1:
            raise Exception(
                f"Found {marker_count} extraction cache markers. Expected exactly 1 "
                "(cache should be reused, not re-extracted)."
            )

    with subtest("--lab-tar without node_local_path errors"):
        config_no_local = f"""
        submission_target = "local"
        [targets.local]
        base_path = "{base_path}"
        default_scheduler = "local"
        default_execution_type = "bwrap"
        [targets.local.local]
        execution_types = ["bwrap"]
        local_concurrency = 4
        """
        machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config_no_local}\nEOF")

        rc, output = machine.execute(
            f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local 2>&1"
        )
        print(f"Exit code without node_local_path: {rc}")
        print(f"Output: {output[:500]}")
        if rc == 0:
            raise Exception("Expected failure with --artifact-store node-local and no node_local_path!")
        if "node_local_path" not in output:
            raise Exception(f"Error message should mention node_local_path, got: {output[:500]}")
        print("Correct: --artifact-store node-local without node_local_path produces clear error")

    print("\n" + "=" * 60)
    print("E2E LOCAL BWRAP LAB-TAR TEST COMPLETED")
    print("=" * 60)
  '';
}
