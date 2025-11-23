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
        virtualisation.diskSize = 8192;
        virtualisation.memorySize = 2048;
        virtualisation.cores = 2;
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
        virtualisation.diskSize = 8192;
        virtualisation.memorySize = 4096;
        virtualisation.cores = 2;

        networking.hostName = "cluster";
        networking.firewall.enable = false;

        virtualisation.docker.enable = true;
        virtualisation.podman.enable = true;
        services.openssh.enable = true;

        environment.systemPackages = [
          repxRunner
          pkgs.bubblewrap
          pkgs.rsync
          pkgs.bash
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

        services.munge.enable = true;

        environment.etc."munge/munge.key" = {
          text = "mungeverryweakkeybuteasytointegratoinatest";
          mode = "0400";
          user = "munge";
          group = "munge";
        };

        systemd.tmpfiles.rules = [
          "d /etc/munge 0700 munge munge -"
        ];

        services.slurm = {
          server.enable = true;
          client.enable = true;
          controlMachine = "cluster";
          procTrackType = "proctrack/pgid";
          nodeName = [ "cluster CPUs=2 RealMemory=3000 State=UNKNOWN" ];
          partitionName = [ "default Nodes=cluster Default=YES MaxTime=INFINITE State=UP" ];
          extraConfig = ''
            SlurmdTimeout=60
            SlurmctldTimeout=60
          '';
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

    def run_slurm_test(runtime):
        print(f"--- Testing Remote Slurm: {runtime} ---")

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

        client.succeed("mkdir -p /root/.config/repx")
        client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

        client.succeed("repx-runner run simulation-run --lab ${referenceLab}")

        cluster.succeed("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")

        cluster.succeed("rm -rf /home/repxuser/repx-store/outputs/*")
        cluster.succeed("rm -rf /home/repxuser/repx-store/cache/*")

    run_slurm_test("native")
    run_slurm_test("bwrap")

    cluster.wait_for_unit("docker.service")
    run_slurm_test("docker")

    run_slurm_test("podman")
  '';
}
