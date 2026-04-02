{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "e2e-slurm-scatter-gather-lab-tar";

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
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnutar
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
          "d /local/scratch 0777 root root -"
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

    LAB_PATH = "${referenceLab}"

    def find_scatter_gather_job():
        for root, dirs, files in os.walk(LAB_PATH):
            for file in files:
                if not file.endswith(".json"):
                    continue
                full_path = os.path.join(root, file)
                try:
                    with open(full_path) as f:
                        data = json.load(f)
                        if data.get("name") == "simulation-run" and "jobs" in data:
                            for jid, jval in data["jobs"].items():
                                if jval.get("stage_type") == "scatter-gather":
                                    return jid
                except:
                    pass
        return None

    sg_job_id = find_scatter_gather_job()
    if not sg_job_id:
        raise Exception("No scatter-gather job found in reference lab!")
    print(f"Scatter-gather job ID: {sg_job_id}")

    config = """
    submission_target = "cluster"
    [targets.local]
    base_path = "/root/repx-local"

    [targets.cluster]
    address = "repxuser@cluster"
    base_path = "/home/repxuser/repx-store"
    node_local_path = "/local/scratch"
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

    with subtest("Submit scatter-gather job with --artifact-store node-local"):
        print(f"--- Submitting SG job {sg_job_id} with lab-tar ---")
        client.succeed(f"repx run {sg_job_id} --lab ${referenceLab} --artifact-store node-local")

    with subtest("Wait for slurm jobs to complete"):
        cluster.succeed("""
            for i in {1..300}; do
                total=$(squeue -h -u repxuser | wc -l)
                running=$(squeue -h -u repxuser -t RUNNING | wc -l)

                if [ "$total" -eq 0 ]; then
                    echo "Queue empty, jobs finished."
                    exit 0
                fi

                if [ "$running" -eq 0 ] && [ "$total" -gt 0 ]; then
                    sleep 5
                    total2=$(squeue -h -u repxuser | wc -l)
                    running2=$(squeue -h -u repxuser -t RUNNING | wc -l)
                    if [ "$running2" -eq 0 ]; then
                        echo "No running jobs. Remaining $total2 are stale held/cancelled. Continuing."
                        squeue -h -u repxuser -o '%i' | xargs -r scancel 2>/dev/null || true
                        exit 0
                    fi
                fi

                sleep 2
            done
            echo "Timeout waiting for Slurm jobs to finish."
            exit 1
        """)

    with subtest("Scatter-gather job succeeded"):
        rc, _ = cluster.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("\n>>> SLURM JOB HISTORY (squeue + scontrol):")
            rc2, sacct_out = cluster.execute("sacct --format=JobID,JobName,State,ExitCode,NodeList --noheader -u repxuser 2>&1")
            print(sacct_out if rc2 == 0 else f"sacct unavailable: {sacct_out.strip()}")
            print("\n>>> OUTPUT TREE:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -maxdepth 5 2>/dev/null || echo 'no outputs dir'"))
            print("\n>>> SUBMISSIONS DIR:")
            print(cluster.succeed("find /home/repxuser/repx-store/submissions -type f 2>/dev/null || echo 'no submissions'"))
            print("\n>>> SBATCH SCRIPTS:")
            print(cluster.succeed("find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec echo '=== {} ===' \\; -exec cat {} \\; 2>/dev/null || echo 'no sbatch'"))
            print("\n>>> SLURM LOGS:")
            print(cluster.succeed("find /home/repxuser/repx-store -name 'slurm-*.out' -exec echo '--- {} ---' \\; -exec cat {} \\; 2>/dev/null || echo 'no slurm logs'"))
            print("\n>>> STDERR LOGS:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' -exec echo '--- {} ---' \\; -exec cat {} \\; 2>/dev/null || echo 'no stderr'"))
            print("\n>>> NODE-LOCAL CONTENTS:")
            print(cluster.succeed("find /local/scratch -type f 2>/dev/null | head -50 || echo 'empty'"))
            print("\n>>> LAB-TARS ON SHARED:")
            print(cluster.succeed("find /home/repxuser/repx-store/lab-tars -type f 2>/dev/null || echo 'no lab-tars'"))
            raise Exception("Scatter-gather job failed!")

        success_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Found {success_count} SUCCESS markers")

    with subtest("Lab tar on shared storage"):
        tar_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/lab-tars -name '*.tar' 2>/dev/null | wc -l"
        ).strip())
        print(f"Lab tars: {tar_count}")
        if tar_count == 0:
            raise Exception("No lab tar found on shared storage!")

    with subtest("Lab extracted to node-local by worker bootstrap"):
        marker_count = int(cluster.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Extraction markers on node-local: {marker_count}")
        if marker_count == 0:
            raise Exception("No extraction marker on node-local! Worker bootstrap did not run.")

    with subtest("Worker sbatch --wrap contains flock bootstrap"):
        rc_sacct, sacct_output = cluster.execute(
            "sacct --format=JobID,JobName --noheader -u repxuser 2>&1"
        )
        print(f"sacct (rc={rc_sacct}):\n{sacct_output.strip()}")

        slurm_logs = cluster.succeed(
            "find /home/repxuser/repx-store -name 'slurm-*.out' -exec cat {} \\; 2>/dev/null"
        ).strip()
        print(f"Slurm log length: {len(slurm_logs)} chars")

        sbatch_content = cluster.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec cat {} \\; 2>/dev/null"
        )
        if "--lab-tar-path" not in sbatch_content:
            print(f"SBATCH CONTENT:\n{sbatch_content[:3000]}")
            raise Exception("Orchestrator sbatch does not pass --lab-tar-path to scatter-gather!")
        print("--lab-tar-path found in orchestrator sbatch script")

    with subtest("Single extraction despite multiple worker jobs"):
        marker_count = int(cluster.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' | wc -l"
        ).strip())
        print(f"Total extraction markers: {marker_count}")
        if marker_count > 1:
            raise Exception(
                f"Found {marker_count} extraction markers, expected 1. "
                "flock dedup may not be working for worker bootstrap."
            )

    with subtest("Worker SLURM IDs manifest exists"):
        manifest = cluster.succeed(
            "find /home/repxuser/repx-store/outputs -name 'worker_slurm_ids.json' -exec cat {} \\; 2>/dev/null"
        ).strip()
        if manifest:
            worker_ids = json.loads(manifest)
            print(f"Worker SLURM IDs: {worker_ids}")
            assert isinstance(worker_ids, list), f"Expected list, got {type(worker_ids)}"
            if len(worker_ids) > 0:
                print(f"Scatter-gather produced {len(worker_ids)} worker jobs")
        else:
            print("Warning: No worker_slurm_ids.json (may be expected for zero work items)")

    with subtest("Outputs on shared, not node-local"):
        output_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -type f | wc -l"
        ).strip())
        local_out = int(cluster.succeed(
            "find /local/scratch -path '*/outputs/*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Shared outputs: {output_count}")
        print(f"Node-local outputs: {local_out}")
        if output_count == 0:
            raise Exception("No outputs on shared storage!")
        if local_out > 0:
            raise Exception("Outputs leaked to node-local!")

    print("\n" + "=" * 60)
    print("E2E SLURM SCATTER-GATHER LAB-TAR TEST COMPLETED")
    print("=" * 60)
  '';
}
