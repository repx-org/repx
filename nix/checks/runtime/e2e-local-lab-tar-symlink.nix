{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "e2e-local-lab-tar-symlink";

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

    import json as _json, os as _os
    def find_simple_leaf_job(lab_path):
        """Find a simple job whose entire dependency chain is also simple."""
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
    job_id = find_simple_leaf_job("${referenceLab}")
    if not job_id:
        raise Exception("No simple leaf job found in reference lab!")
    run_args = job_id
    print(f"Using simple job: {run_args}")

    machine.succeed("ln -s ${referenceLab} /tmp/lab-symlink")

    with subtest("Submit with symlinked lab path and --artifact-store node-local"):
        machine.succeed(f"repx run {run_args} --lab /tmp/lab-symlink --artifact-store node-local")

    with subtest("Jobs succeeded"):
        success_count = int(machine.succeed(
            f"find {base_path}/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"SUCCESS markers: {success_count}")
        if success_count == 0:
            print(machine.succeed(f"find {base_path}/outputs -maxdepth 4"))
            raise Exception("No SUCCESS markers!")

    with subtest("Lab tar was created (from resolved symlink)"):
        tar_files = machine.succeed(
            f"find {base_path} -name '*.tar' -not -path '*/store/*' 2>/dev/null"
        ).strip()
        print(f"Tar files: {tar_files}")
        if not tar_files:
            raise Exception("No tar file created!")

    with subtest("Node-local extraction has real files, not dangling symlinks"):
        lab_entries = machine.succeed(
            f"find {node_local}/repx/labs -maxdepth 3 -type f | head -5 2>/dev/null"
        ).strip()
        print(f"Sample real files in extraction: {lab_entries}")
        if not lab_entries:
            raise Exception("No real files in extraction — tar may have packaged a symlink!")

        host_tools = machine.succeed(
            f"find {node_local}/repx/labs -path '*/host-tools/*/bin/*' -type f -o -path '*/host-tools/*/bin/*' -type l 2>/dev/null | head -3"
        ).strip()
        print(f"Host-tools in extraction: {host_tools}")
        if not host_tools:
            print("WARNING: No host-tools found in extraction")

    with subtest("Extraction marker exists"):
        markers = machine.succeed(
            f"find {node_local}/repx/labs -name '.extracted-*' 2>/dev/null"
        ).strip()
        print(f"Extraction markers: {markers}")
        if not markers:
            raise Exception("No extraction marker!")

    print("\n" + "=" * 60)
    print("E2E LOCAL LAB-TAR SYMLINK TEST COMPLETED")
    print("=" * 60)
  '';
}
