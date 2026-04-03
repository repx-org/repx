{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
in

pkgs.testers.runNixOSTest {
  name = "e2e-internal-logs";

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
    cache_dir = "/root/.cache/repx"

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
    local_concurrency = 1
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed(f"mkdir -p {base_path}/artifacts/host-tools/default/bin")
    machine.succeed(f"ln -s $(which bwrap) {base_path}/artifacts/host-tools/default/bin/bwrap")

    subset_jobs = get_subset_jobs("${referenceLab}")
    if not subset_jobs:
        raise Exception("get_subset_jobs returned empty list")
    run_args = " ".join(subset_jobs)
    print(f"Running subset: {run_args}")

    with subtest("Run produces internal log files"):
        machine.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")

        session_logs = machine.succeed(f"find {cache_dir}/logs -name 'repx_*.log' -type f 2>/dev/null").strip()
        print(f"Session logs: {session_logs}")
        if not session_logs:
            raise Exception("No session log files found in cache dir!")

        internal_logs = machine.succeed(f"find {cache_dir}/logs -name 'repx-internal_*.log' -type f 2>/dev/null").strip()
        print(f"Internal logs: {internal_logs}")
        if not internal_logs:
            raise Exception(
                "No internal log files found! "
                "The repx binary must produce repx-internal_*.log files for each job subprocess."
            )

        internal_count = int(machine.succeed(f"find {cache_dir}/logs -name 'repx-internal_*.log' -type f | wc -l").strip())
        print(f"Internal log count: {internal_count}")
        if internal_count == 0:
            raise Exception("Internal log count is 0!")

    with subtest("Internal logs contain bwrap command"):
        bwrap_matches = int(machine.succeed(
            f"grep -rl 'Full bwrap command' {cache_dir}/logs/repx-internal_*.log 2>/dev/null | wc -l"
        ).strip())
        print(f"Internal logs with 'Full bwrap command': {bwrap_matches}")
        if bwrap_matches == 0:
            first_log = machine.succeed(f"ls {cache_dir}/logs/repx-internal_*.log | head -1").strip()
            if first_log:
                content = machine.succeed(f"cat {first_log}")
                print(f"Content of {first_log}:\n{content[:2000]}")
            raise Exception(
                "No internal log contains 'Full bwrap command'. "
                "The bwrap command must be logged at INFO level by default."
            )

    with subtest("Internal log symlink exists"):
        symlink_exists = machine.succeed(f"test -L {cache_dir}/repx-internal.log && echo yes || echo no").strip()
        print(f"repx-internal.log symlink exists: {symlink_exists}")
        if symlink_exists != "yes":
            raise Exception("repx-internal.log symlink not found in cache dir!")

    with subtest("Session log contains spawned command"):
        cmd_matches = int(machine.succeed(
            f"grep -c '\\[CMD\\]' {cache_dir}/logs/repx_*.log 2>/dev/null || echo 0"
        ).strip())
        print(f"Session log [CMD] lines: {cmd_matches}")
        if cmd_matches == 0:
            raise Exception(
                "Session log does not contain [CMD] lines. "
                "Spawned commands must be logged at INFO level."
            )

    print("\n" + "=" * 60)
    print("E2E INTERNAL LOGS TEST COMPLETED")
    print("=" * 60)
  '';
}
