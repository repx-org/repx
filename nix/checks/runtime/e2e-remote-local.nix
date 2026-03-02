{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-remote-local-test";

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 8172;
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
          memorySize = 8172;
          cores = 4;
          docker.enable = true;
          podman.enable = true;
        };

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
          repx
          pkgs.bubblewrap
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

    import json
    import os

    LAB_PATH = "${referenceLab}"

    exec(open("${./helpers/get-subset-jobs.py}").read())

    subset_jobs = get_subset_jobs(LAB_PATH)
    if not subset_jobs:
        print(f"ERROR: Could not find any jobs for 'simulation-run' in {LAB_PATH}.")
        print(f"Listing files in {LAB_PATH} for debugging:")
        os.system(f"find {LAB_PATH} -maxdepth 4")
        raise Exception("Failed to find subset of jobs. Aborting to prevent running full suite (800+ jobs).")

    run_args = " ".join(subset_jobs)
    print(f"Running subset of jobs: {run_args}")

    def run_remote_test(runtime):
        print(f"--- Testing Remote Local: {runtime} ---")

        config = f"""
        submission_target = "remote"
        [targets.local]
        base_path = "/root/repx-local"
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

        client.succeed(f"repx run {run_args} --lab ${referenceLab}")

        rc, _ = server.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")
        if rc != 0:
            print(f"!!! [{runtime}] TEST FAILED. Dumping debug info:")
            print("\n>>> OUTPUT DIRECTORY TREE:")
            print(server.succeed("find /home/repxuser/repx-store/outputs -maxdepth 4"))
            print("\n>>> STDERR LOGS:")
            print(server.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' -exec echo '--- {} ---' \\; -exec cat {} \\;"))
            raise Exception(f"Run failed for runtime: {runtime}")

        server.succeed("rm -rf /home/repxuser/repx-store/outputs/*")
        server.succeed("rm -rf /home/repxuser/repx-store/cache/*")

    run_remote_test("bwrap")

    server.wait_for_unit("docker.service")
    run_remote_test("docker")

    run_remote_test("podman")
  '';
}
