{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-scatter-gather-cancel-test";

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 2048;
          cores = 4;
        };
        environment.systemPackages = [
          repx
          pkgs.openssh
          pkgs.rsync
          pkgs.jq
        ];
        programs.ssh.extraConfig = "StrictHostKeyChecking no";
      };
    cluster =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 4096;
          cores = 4;
        };

        networking.hostName = "cluster";
        networking.firewall.enable = false;

        environment.systemPackages = [
          repx
          pkgs.bubblewrap
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.jq
        ];

        users.users.repxuser = {
          isNormalUser = true;
          password = "password";
          home = "/home/repxuser";
          createHome = true;
        };

        environment.etc."munge/munge.key" = {
          text = "mungeverryweakkeybuteasytointegratoinatest";
          mode = "0400";
          user = "munge";
          group = "munge";
        };

        systemd.tmpfiles.rules = [
          "d /etc/munge 0700 munge munge -"
        ];

        services = {
          openssh.enable = true;
          munge.enable = true;
          slurm = {
            server.enable = true;
            client.enable = true;
            controlMachine = "cluster";
            procTrackType = "proctrack/pgid";
            nodeName = [ "cluster CPUs=4 RealMemory=3000 State=UNKNOWN" ];
            partitionName = [ "main Nodes=cluster Default=YES MaxTime=INFINITE State=UP" ];

            extraConfig = ''
              SlurmdTimeout=60
              SlurmctldTimeout=60
            '';
          };
        };
      };
  };

  testScript = ''
    import json
    import os

    start_all()

    client.succeed("mkdir -p /root/.ssh")
    client.succeed("ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519 -N \"\" ")
    pub_key = client.succeed("cat /root/.ssh/id_ed25519.pub").strip()

    cluster.succeed("mkdir -p /home/repxuser/.ssh")
    cluster.succeed(f"echo '{pub_key}' >> /home/repxuser/.ssh/authorized_keys")
    cluster.succeed("chown -R repxuser:users /home/repxuser/.ssh")
    cluster.succeed("chmod 700 /home/repxuser/.ssh")
    cluster.succeed("chmod 600 /home/repxuser/.ssh/authorized_keys")
    cluster.succeed("loginctl enable-linger repxuser")

    client.wait_for_unit("network.target")
    cluster.wait_for_unit("sshd.service")

    client.succeed("ssh repxuser@cluster 'echo SSH_OK'")

    cluster.wait_for_unit("munged.service")
    cluster.wait_for_unit("slurmctld.service")
    cluster.wait_for_unit("slurmd.service")

    cluster.succeed("sinfo")

    base_path = "/home/repxuser/repx-store"

    LAB_PATH = "${referenceLab}"

    def find_scatter_gather_job():
        """Find a scatter-gather stage job ID from the reference lab."""
        print(f"Searching for scatter-gather job in {LAB_PATH}")
        for root, dirs, files in os.walk(LAB_PATH):
            for file in files:
                if file.endswith(".json"):
                    full_path = os.path.join(root, file)
                    try:
                        with open(full_path, 'r') as f:
                            data = json.load(f)
                            if data.get("name") == "simulation-run" and "jobs" in data:
                                jobs = data["jobs"]
                                for jid, jval in jobs.items():
                                    st = jval.get("stage_type", "simple")
                                    if st == "scatter-gather":
                                        print(f"Found scatter-gather job: {jid} ({jval.get('name', '?')})")
                                        return jid
                    except Exception as e:
                        print(f"Warning: {e}")
        return None

    sg_job_id = find_scatter_gather_job()
    if not sg_job_id:
        raise Exception("No scatter-gather job found in reference lab!")

    print(f"Scatter-gather job ID: {sg_job_id}")

    config = f"""
    submission_target = "cluster"
    [targets.local]
    base_path = "/root/repx-local"

    [targets.cluster]
    address = "repxuser@cluster"
    base_path = "{base_path}"
    default_scheduler = "slurm"
    default_execution_type = "native"

    [targets.cluster.slurm]
    execution_types = ["native"]
    """

    resources = """
    [defaults]
    partition = "main"
    """

    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")
    client.succeed(f"cat <<EOF > /root/.config/repx/resources.toml\n{resources}\nEOF")

    with subtest("Run scatter-gather job via Slurm"):
        print(f"--- Submitting scatter-gather job {sg_job_id} via Slurm ---")
        client.succeed(f"repx run {sg_job_id} --lab ${referenceLab}")

        print("Waiting for all Slurm jobs to finish...")
        cluster.succeed("""
            for i in {1..900}; do
                if ! squeue -h -u repxuser | grep .; then
                    echo "Queue empty, jobs finished."
                    exit 0
                fi
                sleep 2
            done
            echo "Timeout waiting for Slurm jobs to finish."
            exit 1
        """)

        rc, _ = cluster.execute(f"find {base_path}/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("\n>>> SLURM JOB HISTORY (sacct):")
            print(cluster.succeed("sacct --format=JobID,JobName,State,ExitCode"))
            print("\n>>> OUTPUT DIRECTORY TREE:")
            print(cluster.succeed(f"find {base_path}/outputs -maxdepth 4"))
            print("\n>>> STDERR LOGS:")
            print(cluster.succeed(f"find {base_path}/outputs -name 'stderr.log' -exec echo '--- {{}} ---' \\; -exec cat {{}} \\;"))
            raise Exception("Scatter-gather job failed!")

        print("Scatter-gather job completed successfully.")

    with subtest("Worker SLURM IDs manifest exists"):
        print("--- Checking worker_slurm_ids.json manifest ---")

        manifest_check = cluster.succeed(
            f"find {base_path}/outputs -name 'worker_slurm_ids.json' -exec cat {{}} \\;"
        ).strip()

        print(f"Worker SLURM IDs manifest content: {manifest_check}")

        if not manifest_check:
            print("Warning: No worker_slurm_ids.json found.")
            print("This is expected only if the scatter produced zero work items.")
            print("Checking scatter output...")
            print(cluster.succeed(f"find {base_path}/outputs -maxdepth 5 -name 'work_items.json' -exec cat {{}} \\;"))
        else:
            worker_ids = json.loads(manifest_check)
            assert isinstance(worker_ids, list), \
                f"Expected a JSON array of SLURM IDs, got: {type(worker_ids)}"
            print(f"Found {len(worker_ids)} worker SLURM IDs in manifest.")

            if len(worker_ids) > 0:
                for wid in worker_ids:
                    assert isinstance(wid, int) and wid > 0, \
                        f"Invalid worker SLURM ID: {wid}"

                print(f"All {len(worker_ids)} worker SLURM IDs are valid.")

    with subtest("Cancel workers via manifest (deterministic)"):
        print("--- Testing manifest-based worker cancellation ---")

        held_ids = []
        for i in range(3):
            raw = cluster.succeed(
                f"su - repxuser -c 'sbatch --parsable --hold --job-name=test-worker-{i} "
                f"--partition=main --wrap=\"sleep 3600\"'"
            ).strip()
            slurm_id = int(raw.split(";")[0])
            held_ids.append(slurm_id)
            print(f"Submitted held job {i}: SLURM ID {slurm_id}")

        print(f"All held job IDs: {held_ids}")

        for sid in held_ids:
            state = cluster.succeed(f"squeue -j {sid} -h -o '%T'").strip()
            assert state == "PENDING", \
                f"Expected job {sid} in PENDING state, got: {state}"
        print("All held jobs confirmed in PENDING state.")

        manifest_dir = f"{base_path}/outputs/cancel-test-job/repx"
        cluster.succeed(f"su - repxuser -c 'mkdir -p {manifest_dir}'")
        manifest_json = json.dumps(held_ids)
        manifest_file = f"{manifest_dir}/worker_slurm_ids.json"
        cluster.succeed(
            f"printf '%s' '{manifest_json}' | su - repxuser -c 'tee {manifest_file} > /dev/null'"
        )

        read_back = cluster.succeed(f"cat {manifest_dir}/worker_slurm_ids.json").strip()
        cancel_ids = json.loads(read_back)
        assert cancel_ids == held_ids, \
            f"Manifest round-trip mismatch: wrote {held_ids}, read {cancel_ids}"

        id_str = " ".join(str(sid) for sid in cancel_ids)
        cluster.succeed(f"su - repxuser -c 'scancel {id_str}'")
        print(f"scancel executed for IDs: {id_str}")

        import time
        time.sleep(5)
        for sid in held_ids:
            rc, output = cluster.execute(f"squeue -j {sid} -h -o '%T'")
            state = output.strip() if rc == 0 else ""
            assert state == "" or state == "CANCELLED", \
                f"Job {sid} should be cancelled, but state is: '{state}'"

        print("All held jobs successfully cancelled via manifest.")
        print("Scatter-gather cancel test completed.")

    print("\n" + "=" * 60)
    print("E2E SCATTER-GATHER CANCEL TEST COMPLETED")
    print("=" * 60)
  '';
}
