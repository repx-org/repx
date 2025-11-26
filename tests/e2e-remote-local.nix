{
  pkgs,
  repxRunner,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-remote-local-test";

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation.diskSize = 8172;
        virtualisation.memorySize = 8172;
        virtualisation.cores = 2;
        environment.systemPackages = [
          repxRunner
          pkgs.openssh
          pkgs.rsync
        ];
        programs.ssh.extraConfig = ''
          StrictHostKeyChecking no
          ControlMaster auto
          ControlPath ~/.ssh/master-%r@%h:%p
          ControlPersist 60
        '';
      };

    server =
      { pkgs, ... }:
      {
        virtualisation.diskSize = 8172;
        virtualisation.memorySize = 8172;
        virtualisation.cores = 4;
        virtualisation.docker.enable = true;
        virtualisation.podman.enable = true;

        networking.dhcpcd.denyInterfaces = [
          "veth*"
          "docker*"
          "podman*"
        ];

        services.openssh = {
          enable = true;
          settings.MaxStartups = "100:30:500";
        };

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
      };
  };

  testScript = ''
    start_all()

    client.succeed("mkdir -p /root/.ssh")
    client.succeed("ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519 -N \"\" ")

    pub_key = client.succeed("cat /root/.ssh/id_ed25519.pub").strip()
    server.succeed("mkdir -p /home/repxuser/.ssh")
    server.succeed(f"echo '{pub_key}' >> /home/repxuser/.ssh/authorized_keys")
    server.succeed("chown -R repxuser:users /home/repxuser/.ssh")
    server.succeed("chmod 700 /home/repxuser/.ssh")
    server.succeed("chmod 600 /home/repxuser/.ssh/authorized_keys")

    client.wait_for_unit("network.target")
    server.wait_for_unit("sshd.service")
    client.succeed("ssh repxuser@server 'echo SSH_OK'")

    def run_remote_test(runtime):
        print(f"--- Testing Remote Local: {runtime} ---")

        config = f"""
        submission_target = "remote"
        [targets.remote]
        address = "repxuser@server"
        base_path = "/home/repxuser/repx-store"
        default_scheduler = "local"
        default_execution_type = "{runtime}"
        [targets.remote.local]
        execution_types = ["{runtime}"]
        local_concurrency = 2
        """

        client.succeed("mkdir -p /root/.config/repx")
        client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

        client.succeed("repx-runner run simulation-run --lab ${referenceLab}")

        server.succeed("grep -rE '400|415' /home/repxuser/repx-store/outputs/*/out/total_sum.txt")

        server.succeed("rm -rf /home/repxuser/repx-store/outputs/*")
    server.succeed("rm -rf /home/repxuser/repx-store/cache/*")

    # run_remote_test("native") # no native support on remote
    run_remote_test("bwrap")

    server.wait_for_unit("docker.service")
    run_remote_test("docker")

    run_remote_test("podman")
  '';
}
