{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-incremental-sync-test";

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 4096;
          cores = 2;
        };
        environment.systemPackages = [
          repx
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
        virtualisation = {
          diskSize = 25600;
          memorySize = 4096;
          cores = 2;
          docker.enable = true;
        };

        networking.dhcpcd.denyInterfaces = [
          "veth*"
          "docker*"
        ];
        services.openssh.enable = true;

        environment.systemPackages = [
          repx
          pkgs.bubblewrap
          pkgs.bash
          pkgs.gnutar
          pkgs.rsync
        ];

        users.users.repxuser = {
          isNormalUser = true;
          extraGroups = [ "docker" ];
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

    config = """
    submission_target = "remote"
    [targets.local]
    base_path = "/root/repx-local"
    [targets.remote]
    address = "repxuser@server"
    base_path = "/home/repxuser/repx-store"
    default_scheduler = "local"
    default_execution_type = "docker"
    [targets.remote.local]
    execution_types = ["docker"]
    local_concurrency = 2
    """
    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    print("--- 3. First Sync & Run ---")
    client.succeed("repx run simulation-run --lab ${referenceLab}")

    server.succeed("ls -R /home/repxuser/repx-store/artifacts/images")

    print("--- 4. Sabotage: Deleting a layer ---")
    layers = server.succeed("find /home/repxuser/repx-store/artifacts/images -name 'layer.tar'").splitlines()
    if not layers:
        raise Exception("No layers found on server!")
    victim_layer = layers[0]
    print(f"Deleting layer: {victim_layer}")
    server.succeed(f"rm {victim_layer}")


    server.succeed("rm -rf /home/repxuser/repx-store/outputs/*")
    server.succeed("rm -rf /home/repxuser/repx-store/cache/*")

    print("--- 5. Second Sync & Run ---")
    client.succeed("repx run simulation-run --lab ${referenceLab}")

    print("--- 6. Verify Restoration ---")
    server.succeed(f"test -f {victim_layer}")
    print("Layer was successfully restored!")

  '';
}
