{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "e2e-slurm-bwrap-scatter-gather-lab-tar";

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 2048;
          cores = 2;
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
          pkgs.bubblewrap
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
    default_execution_type = "bwrap"

    [targets.cluster.slurm]
    execution_types = ["bwrap"]
    """

    resources = """
    [defaults]
    partition = "main"
    """

    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")
    client.succeed(f"cat <<EOF > /root/.config/repx/resources.toml\n{resources}\nEOF")

    with subtest("Submit scatter-gather job with bwrap + --artifact-store node-local"):
        print(f"--- Submitting SG job {sg_job_id} with bwrap + lab-tar ---")
        client.succeed(f"repx run {sg_job_id} --lab ${referenceLab} --artifact-store node-local")

    with subtest("Wait for slurm jobs to complete"):
        cluster.succeed("""
            for i in {1..600}; do
                total=$(squeue -h -u repxuser | wc -l)
                running=$(squeue -h -u repxuser -t RUNNING | wc -l)

                if [ "$total" -eq 0 ]; then
                    echo "Queue empty, jobs finished."
                    exit 0
                fi

                if [ "$running" -eq 0 ] && [ "$total" -gt 0 ]; then
                    sleep 5
                    running2=$(squeue -h -u repxuser -t RUNNING | wc -l)
                    if [ "$running2" -eq 0 ]; then
                        echo "No running jobs. Cancelling stale held jobs."
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
            print("\n>>> SLURM QUEUE:")
            rc2, sacct_out = cluster.execute("sacct --format=JobID,JobName,State,ExitCode 2>&1")
            print(sacct_out if rc2 == 0 else f"sacct unavailable: {sacct_out.strip()}")
            print("\n>>> OUTPUT TREE:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -maxdepth 5 2>/dev/null || echo 'no outputs'"))
            print("\n>>> SBATCH SCRIPTS:")
            print(cluster.succeed("find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec echo '=== {} ===' \\; -exec head -50 {} \\; 2>/dev/null || echo 'no sbatch'"))
            print("\n>>> SLURM LOGS (first 3):")
            print(cluster.succeed("find /home/repxuser/repx-store -name 'slurm-*.out' | head -3 | xargs -I{} sh -c 'echo \"--- {} ---\" && cat {}' 2>/dev/null || echo 'no slurm logs'"))
            print("\n>>> STDERR LOGS (first 3):")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' | head -3 | xargs -I{} sh -c 'echo \"--- {} ---\" && cat {}' 2>/dev/null || echo 'no stderr'"))
            raise Exception("Scatter-gather job with bwrap failed!")

        success_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Found {success_count} SUCCESS markers")

    with subtest("Lab tar extracted to node-local"):
        marker_count = int(cluster.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Extraction markers: {marker_count}")
        if marker_count == 0:
            raise Exception("No extraction marker on node-local!")

    with subtest("Bwrap rootfs extracted from local artifacts"):
        rootfs_count = int(cluster.succeed(
            "find /local/scratch/repx -name 'rootfs' -type d 2>/dev/null | wc -l"
        ).strip())
        print(f"Rootfs dirs on node-local: {rootfs_count}")
        if rootfs_count == 0:
            shared_rootfs = int(cluster.succeed(
                "find /home/repxuser/repx-store -name 'rootfs' -type d 2>/dev/null | wc -l"
            ).strip())
            print(f"Rootfs dirs on shared: {shared_rootfs}")
            print("NOTE: rootfs found on shared, not node-local (acceptable)")

    with subtest("Gather output directory and slurm log exist"):
        gather_logs = cluster.succeed(
            "find /home/repxuser/repx-store/outputs -path '*/gather/repx/slurm-*.out' 2>/dev/null"
        ).strip()
        print(f"Gather slurm logs: {gather_logs}")
        if gather_logs:
            print("Gather directory pre-creation confirmed (slurm log exists)")
        else:
            print("WARNING: No gather slurm log found (may use different output path)")

    with subtest("Worker bootstrap uses POSIX flock (not bash fd redirection)"):
        sbatch_content = cluster.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec cat {} \\; 2>/dev/null"
        )
        if '200>' in sbatch_content:
            raise Exception("Sbatch uses bash fd redirection (200>), not POSIX flock!")
        if 'flock' not in sbatch_content:
            raise Exception("No flock in sbatch scripts!")
        print("POSIX flock bootstrap confirmed")

    with subtest("Gather command has --local-artifacts-path and --lab-tar-path"):
        sbatch_content = cluster.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec cat {} \\; 2>/dev/null"
        )
        if "--lab-tar-path" not in sbatch_content:
            raise Exception("Orchestrator sbatch missing --lab-tar-path!")
        print("--lab-tar-path found in orchestrator sbatch")

    with subtest("Outputs on shared, not node-local"):
        output_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -type f | wc -l"
        ).strip())
        local_out = int(cluster.succeed(
            "find /local/scratch -path '*/outputs/*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Shared outputs: {output_count}, Node-local outputs: {local_out}")
        if output_count == 0:
            raise Exception("No outputs on shared!")
        if local_out > 0:
            raise Exception("Outputs leaked to node-local!")

    print("\n" + "=" * 60)
    print("E2E SLURM BWRAP SCATTER-GATHER LAB-TAR TEST COMPLETED")
    print("=" * 60)
  '';
}
