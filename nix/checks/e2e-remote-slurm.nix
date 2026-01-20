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

    client.wait_for_unit("network.target")
    cluster.wait_for_unit("sshd.service")

    client.succeed("ssh repxuser@cluster 'echo SSH_OK'")

    cluster.wait_for_unit("munged.service")
    cluster.wait_for_unit("slurmctld.service")
    cluster.wait_for_unit("slurmd.service")

    cluster.succeed("sinfo")

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
        client.succeed("repx-runner run simulation-run --lab ${referenceLab}")

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
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'slurm-*.out' -exec echo '--- {} ---' \; -exec cat {} \;"))

            print("\n>>> REPX STDERR LOGS (Execution Errors):")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' -exec echo '--- {} ---' \; -exec cat {} \;"))

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
