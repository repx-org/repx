{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
in

pkgs.testers.runNixOSTest {
  name = "e2e-local-bwrap-node-local-path";

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
        raise Exception("get_subset_jobs returned empty list for e2e-local-bwrap-node-local-path")
    run_args = " ".join(subset_jobs)
    print(f"Running subset: {run_args}")

    with subtest("Jobs succeed with node_local_path"):
        print("--- Testing bwrap with node_local_path ---")

        machine.succeed(f"repx run {run_args} --lab ${referenceLab}")

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

    with subtest("Container cache lives on node-local storage"):
        local_cache = machine.succeed(f"find {node_local}/repx/cache/images -name SUCCESS 2>/dev/null | wc -l").strip()
        shared_cache = machine.succeed(f"find {base_path}/cache/images -name SUCCESS 2>/dev/null | wc -l").strip()
        print(f"Image cache SUCCESS markers on node-local: {local_cache}")
        print(f"Image cache SUCCESS markers on shared: {shared_cache}")

        if int(local_cache) == 0:
            raise Exception("No image cache found on node-local storage! node_local_path not used for cache.")
        if int(shared_cache) > 0:
            raise Exception("Image cache found on shared storage -- should be on node-local!")

    with subtest("Cache is reused across jobs (single extraction per image)"):
        image_count = int(machine.succeed(f"find {node_local}/repx/cache/images -name SUCCESS | wc -l").strip())
        print(f"Unique extracted images on node-local: {image_count}")
        print(f"Jobs that succeeded: {success_count}")

        if image_count >= success_count and success_count > 1:
            raise Exception(
                f"Found {image_count} extracted images for {success_count} jobs. "
                "Expected fewer images than jobs (cache reuse)."
            )
        print(f"Cache reuse confirmed: {image_count} image(s) shared across {success_count} job(s)")

    print("\n" + "=" * 60)
    print("E2E LOCAL BWRAP NODE-LOCAL-PATH TEST COMPLETED")
    print("=" * 60)
  '';
}
