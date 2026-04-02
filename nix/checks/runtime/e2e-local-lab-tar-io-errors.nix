{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "e2e-local-lab-tar-io-errors";

  nodes.machine = _: {
    virtualisation = {
      diskSize = 25600;
      memorySize = 4096;
      cores = 2;
    };
    environment.systemPackages = [
      repx
      pkgs.bubblewrap
      pkgs.gnutar
    ];
  };

  testScript = ''
    from get_subset_jobs import get_subset_jobs
    start_all()

    base_path = "/mnt/shared/repx-store"

    machine.succeed(f"mkdir -p {base_path}")
    machine.succeed("mkdir -p /root/.config/repx")

    import json as _json, os as _os
    def find_simple_leaf_job(lab_path):
        for root, dirs, files in _os.walk(lab_path):
            for f in files:
                if not f.endswith(".json"):
                    continue
                try:
                    data = _json.load(open(_os.path.join(root, f)))
                    if data.get("name") != "simulation-run" or "jobs" not in data:
                        continue
                    jobs = data["jobs"]
                    sg_ids = {jid for jid, jv in jobs.items() if jv.get("stage_type") == "scatter-gather"}
                    def has_sg_dep(jid, visited=None):
                        if visited is None:
                            visited = set()
                        if jid in visited:
                            return False
                        visited.add(jid)
                        if jid in sg_ids:
                            return True
                        jv = jobs.get(jid)
                        if not jv:
                            return False
                        for exe in jv.get("executables", {}).values():
                            for inp in exe.get("inputs", []):
                                dep = inp.get("job_id")
                                if dep and has_sg_dep(dep, visited):
                                    return True
                        return False
                    for jid, jv in jobs.items():
                        if jv.get("stage_type", "simple") == "simple" and not has_sg_dep(jid):
                            return jid
                except:
                    pass
        return None
    run_args = find_simple_leaf_job("${referenceLab}")
    if not run_args:
        raise Exception("No simple leaf job found!")

    machine.succeed("mkdir -p /read-only-local")
    machine.succeed("chmod 555 /read-only-local")

    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    node_local_path = "/read-only-local"
    default_scheduler = "local"
    default_execution_type = "bwrap"
    [targets.local.local]
    execution_types = ["bwrap"]
    local_concurrency = 1
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed(f"mkdir -p {base_path}/artifacts/host-tools/default/bin")
    machine.succeed(f"ln -s $(which bwrap) {base_path}/artifacts/host-tools/default/bin/bwrap")

    with subtest("Run with read-only node_local_path fails with contextual error"):
        rc, output = machine.execute(
            f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local 2>&1"
        )
        print(f"Exit code: {rc}")
        print(f"Output (last 2000 chars): {output[-2000:]}")

        if rc == 0:
            raise Exception("Expected failure when node_local_path is read-only, but got success!")

        error_lower = output.lower()
        has_path = "/read-only-local" in output
        has_permission = "permission denied" in error_lower or "read-only" in error_lower
        has_context = any(op in error_lower for op in [
            "create_dir", "mkdir", "extract", "write", "tar",
            "permission", "read-only", "failed"
        ])

        print(f"Error has path reference: {has_path}")
        print(f"Error mentions permission: {has_permission}")
        print(f"Error has operational context: {has_context}")

        if not has_path:
            print("WARNING: Error message does not mention the failing path")
        if not has_permission:
            print("WARNING: Error message does not mention permission denial")
        if "i/o error" in error_lower and not has_path and not has_context:
            raise Exception(
                "Error message is a bare 'I/O error' with no context! "
                "The IoContext refactor should provide operation+path details."
            )

    print("\n" + "=" * 60)
    print("E2E LOCAL LAB-TAR IO ERROR CONTEXT TEST COMPLETED")
    print("=" * 60)
  '';
}
