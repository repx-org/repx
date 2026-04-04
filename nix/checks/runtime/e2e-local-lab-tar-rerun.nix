{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
in

pkgs.testers.runNixOSTest {
  name = "e2e-local-lab-tar-rerun";

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
      pkgs.gnutar
    ];
  };

  testScript = ''
    from get_subset_jobs import get_subset_jobs
    import time
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
        raise Exception("get_subset_jobs returned empty")
    run_args = " ".join(subset_jobs)
    print(f"Running subset: {run_args}")

    with subtest("Run 1: jobs succeed with --artifact-store node-local"):
        machine.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")

        success_1 = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
        print(f"Run 1: {success_1} SUCCESS markers")
        if success_1 == 0:
            raise Exception("Run 1 failed: no SUCCESS markers!")

    with subtest("Run 1: tar was created"):
        tar_files_1 = machine.succeed(
            f"find {base_path}/repx/temp -maxdepth 1 -name '*.tar' 2>/dev/null"
        ).strip()
        print(f"Run 1 local tar files: {tar_files_1}")
        if not tar_files_1:
            raise Exception("No tar file created in repx/temp after Run 1!")

    with subtest("Run 1: extraction cache marker exists"):
        markers_1 = machine.succeed(
            f"find {node_local}/repx/labs -maxdepth 1 -name '.*.repx-cache.json' 2>/dev/null"
        ).strip()
        print(f"Run 1 cache markers: {markers_1}")
        if not markers_1:
            raise Exception("No extraction cache marker after Run 1!")

    tar_stat_1 = machine.succeed(
        f"find {base_path}/repx/temp -maxdepth 1 -name '*.tar' -exec stat --format='%Y %n' {{}} \\; 2>/dev/null | sort"
    ).strip()
    marker_stat_1 = machine.succeed(
        f"find {node_local}/repx/labs -maxdepth 1 -name '.*.repx-cache.json' -exec stat --format='%Y %n' {{}} \\; 2>/dev/null | sort"
    ).strip()
    print(f"Tar stat after run 1: {tar_stat_1}")
    print(f"Marker stat after run 1: {marker_stat_1}")

    machine.succeed(f"rm -rf {base_path}/outputs/*")

    time.sleep(2)

    with subtest("Run 2: re-run same jobs with --artifact-store node-local"):
        machine.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")

        success_2 = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
        print(f"Run 2: {success_2} SUCCESS markers")
        if success_2 == 0:
            raise Exception("Run 2 failed: no SUCCESS markers!")

    with subtest("Tar was NOT re-created (content-hash dedup)"):
        tar_stat_2 = machine.succeed(
            f"find {base_path}/repx/temp -maxdepth 1 -name '*.tar' -exec stat --format='%Y %n' {{}} \\; 2>/dev/null | sort"
        ).strip()
        print(f"Tar stat after run 2: {tar_stat_2}")

        if tar_stat_1 != tar_stat_2:
            raise Exception(
                f"Tar file was re-created! mtime changed.\n"
                f"Before: {tar_stat_1}\nAfter:  {tar_stat_2}"
            )
        print("Tar file was reused (same mtime) - content hash dedup works!")

    with subtest("Extraction was NOT re-done (cache-marker-based dedup)"):
        marker_stat_2 = machine.succeed(
            f"find {node_local}/repx/labs -maxdepth 1 -name '.*.repx-cache.json' -exec stat --format='%Y %n' {{}} \\; 2>/dev/null | sort"
        ).strip()
        print(f"Marker stat after run 2: {marker_stat_2}")

        if marker_stat_1 != marker_stat_2:
            raise Exception(
                f"Extraction cache marker was re-created!\n"
                f"Before: {marker_stat_1}\nAfter:  {marker_stat_2}"
            )
        print("Extraction was skipped (same cache marker mtime) - dedup works!")

    with subtest("Still exactly 1 extraction cache marker"):
        marker_count = int(machine.succeed(
            f"find {node_local}/repx/labs -maxdepth 1 -name '.*.repx-cache.json' | wc -l"
        ).strip())
        print(f"Total extraction cache markers: {marker_count}")
        if marker_count != 1:
            raise Exception(f"Expected 1 extraction cache marker, found {marker_count}!")

    print("\n" + "=" * 60)
    print("E2E LOCAL LAB-TAR RERUN IDEMPOTENCY TEST COMPLETED")
    print("=" * 60)
  '';
}
