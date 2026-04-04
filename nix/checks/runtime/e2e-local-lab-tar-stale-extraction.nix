{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "e2e-local-lab-tar-stale-extraction";

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
        raise Exception("No simple leaf job found!")
    run_args = job_id
    print(f"Using simple job: {run_args}")

    with subtest("Run 1: initial run succeeds"):
        machine.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")
        success_1 = int(machine.succeed(
            f"find {base_path}/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Run 1: {success_1} SUCCESS markers")
        if success_1 == 0:
            raise Exception("Run 1 produced no SUCCESS markers!")

    with subtest("Verify extraction marker exists"):
        markers = machine.succeed(
            f"find {node_local}/repx/labs -name '.extracted-*' 2>/dev/null"
        ).strip()
        print(f"Extraction markers: {markers}")
        if not markers:
            raise Exception("No extraction marker after Run 1!")

    with subtest("Simulate interrupted extraction: remove marker, make files read-only"):
        machine.succeed(
            f"find {node_local}/repx/labs -name '.extracted-*' -delete"
        )
        print("Removed extraction markers")

        machine.succeed(
            f"find {node_local}/repx -name 'SUCCESS' -path '*/labs/*' -delete 2>/dev/null || true"
        )
        print("Removed cache sidecar files")

        machine.succeed(
            f"find {node_local}/repx/labs -type f -exec chmod 444 {{}} \\;"
        )
        machine.succeed(
            f"find {node_local}/repx/labs -type d -exec chmod 555 {{}} \\;"
        )
        print("Made all extracted files and directories read-only")

        file_count = int(machine.succeed(
            f"find {node_local}/repx/labs -type f 2>/dev/null | wc -l"
        ).strip())
        print(f"Read-only files in stale extraction: {file_count}")
        if file_count == 0:
            raise Exception("Extraction dir is empty -- nothing to test!")

        rc, _ = machine.execute(
            f"rm -rf {node_local}/repx/labs/*/store 2>&1"
        )
        leftover = int(machine.succeed(
            f"find {node_local}/repx/labs -type f 2>/dev/null | wc -l"
        ).strip())
        print(f"Files remaining after attempted rm: {leftover} (expected > 0)")

    with subtest("Remove output markers to force re-run"):
        machine.succeed(f"find {base_path}/outputs -name SUCCESS -delete")

    with subtest("Run 2: re-run succeeds despite stale read-only extraction"):
        machine.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")
        success_2 = int(machine.succeed(
            f"find {base_path}/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Run 2: {success_2} SUCCESS markers")
        if success_2 == 0:
            raise Exception(
                "Run 2 failed! repx could not recover from stale read-only extraction."
            )

    with subtest("New extraction marker exists"):
        new_markers = machine.succeed(
            f"find {node_local}/repx/labs -name '.extracted-*' 2>/dev/null"
        ).strip()
        print(f"New extraction markers: {new_markers}")
        if not new_markers:
            raise Exception("No extraction marker after recovery run!")

    print("\n" + "=" * 60)
    print("E2E LOCAL LAB-TAR STALE EXTRACTION RECOVERY TEST COMPLETED")
    print("=" * 60)
  '';
}
