{
  pkgs,
  repxRunner,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-remote-slurm-test";

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 8192;
          memorySize = 2048;
          cores = 4;
        };
        environment.systemPackages = [
          repxRunner
          pkgs.openssh
          pkgs.rsync
        ];
        programs.ssh.extraConfig = "StrictHostKeyChecking no";
      };
    cluster =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 8192;
          memorySize = 4096;
          cores = 4;
          docker.enable = true;
          podman.enable = true;
        };

        networking.hostName = "cluster";
        networking.firewall.enable = false;

        environment.systemPackages = [
          repxRunner
          pkgs.bubblewrap
          pkgs.rsync
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
        ];

        users.users.repxuser = {
          isNormalUser = true;
          extraGroups = [
            "docker"
            "podman"
          ];
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

    import json
    import os

    LAB_PATH = "${referenceLab}"

    def get_subset_jobs():
        print(f"Searching for jobs in {LAB_PATH}")
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
                                    if "workload-generator" in jval.get("name", ""):
                                        print(f"Found workload-generator job: {jid}")
                                        return [jid]

                                if jobs:
                                    first_job = list(jobs.keys())[0]
                                    print(f"Workload generator not found. Selecting first available job: {first_job}")
                                    return [first_job]
                    except Exception as e:
                        print(f"Warning: Failed to read or parse {full_path}: {e}")
        return []

    subset_jobs = get_subset_jobs()
    if not subset_jobs:
        print(f"ERROR: Could not find any jobs for 'simulation-run' in {LAB_PATH}.")
        print(f"Listing files in {LAB_PATH} for debugging:")
        os.system(f"find {LAB_PATH} -maxdepth 4")
        raise Exception("Failed to find subset of jobs. Aborting to prevent running full suite (800+ jobs).")

    run_args = " ".join(subset_jobs)
    print(f"Running subset of jobs: {run_args}")

    def run_slurm_test(runtime):
        print(f"\n>>> Testing Remote Slurm Runtime: {runtime} <<<")

        config = f"""
        submission_target = "cluster"
        [targets.cluster]
        address = "repxuser@cluster"
        base_path = "/home/repxuser/repx-store"
        default_scheduler = "slurm"
        default_execution_type = "{runtime}"

        [targets.cluster.slurm]
        execution_types = ["{runtime}"]
        """

        resources = """
        [defaults]
        partition = "main"
        """

        client.succeed("mkdir -p /root/.config/repx")
        client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")
        client.succeed(f"cat <<EOF > /root/.config/repx/resources.toml\n{resources}\nEOF")

        print(f"[{runtime}] Submitting jobs...")
        client.succeed(f"repx-runner run {run_args} --lab ${referenceLab}")

        print(f"[{runtime}] Waiting for jobs to finish in Slurm queue...")

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

        print(f"[{runtime}] Verifying output...")

        rc, _ = cluster.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")

        if rc != 0:
            print(f"!!! [{runtime}] TEST FAILED. Dumping debug info:")

            print("\n>>> SLURM JOB HISTORY (sacct):")
            print(cluster.succeed("sacct --format=JobID,JobName,State,ExitCode"))

            print("\n>>> SLURM NODE STATE (sinfo):")
            print(cluster.succeed("sinfo"))

            print("\n>>> OUTPUT DIRECTORY TREE:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -maxdepth 4"))

            print("\n>>> SLURM OUTPUT LOGS (Standard Out):")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'slurm-*.out' -exec echo '--- {} ---' \\; -exec cat {} \\;"))

            print("\n>>> REPX STDERR LOGS (Execution Errors):")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' -exec echo '--- {} ---' \\; -exec cat {} \\;"))

            raise Exception(f"Run failed for runtime: {runtime}")
        else:
            print(f"[{runtime}] Success! Jobs completed successfully.")

        cluster.succeed("rm -rf /home/repxuser/repx-store/outputs/*")
        cluster.succeed("rm -rf /home/repxuser/repx-store/cache/*")

    run_slurm_test("native")
    run_slurm_test("bwrap")

    cluster.wait_for_unit("docker.service")
    run_slurm_test("docker")

    run_slurm_test("podman")
  '';
}
