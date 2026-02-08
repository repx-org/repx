{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "non-nixos-remote";

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
          pkgs.jq
        ];
        programs.ssh.extraConfig = ''
          StrictHostKeyChecking no
          ControlMaster auto
          ControlPath ~/.ssh/master-%r@%h:%p
          ControlPersist 60
        '';
      };

    target =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 4096;
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
    import json

    start_all()

    client.succeed("mkdir -p /root/.ssh")
    client.succeed("ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519 -N \"\" ")

    pub_key = client.succeed("cat /root/.ssh/id_ed25519.pub").strip()
    target.succeed("mkdir -p /home/repxuser/.ssh")
    target.succeed(f"echo '{pub_key}' >> /home/repxuser/.ssh/authorized_keys")
    target.succeed("chown -R repxuser:users /home/repxuser/.ssh")
    target.succeed("chmod 700 /home/repxuser/.ssh")
    target.succeed("chmod 600 /home/repxuser/.ssh/authorized_keys")

    client.wait_for_unit("network.target")
    target.wait_for_unit("sshd.service")
    client.succeed("ssh repxuser@target 'echo SSH_OK'")

    base_path = "/home/repxuser/repx-store"

    target.succeed(f"mkdir -p {base_path}/artifacts/host-tools/default/bin")
    target.succeed(f"ln -s $(which bwrap) {base_path}/artifacts/host-tools/default/bin/bwrap")
    target.succeed(f"ln -s $(which docker) {base_path}/artifacts/host-tools/default/bin/docker")
    target.succeed(f"ln -s $(which podman) {base_path}/artifacts/host-tools/default/bin/podman")

    target.succeed(f"chown -R repxuser:users {base_path}")

    target.succeed("mkdir -p /tmp/host-data")
    target.succeed("echo 'HOST_SECRET_DATA' > /tmp/host-data/secret.txt")
    target.succeed("chmod -R a+r /tmp/host-data")

    LAB_PATH = "${referenceLab}"

    def get_subset_jobs():
        print(f"Searching for jobs in {LAB_PATH} (on client)")

        files = client.succeed(f"find {LAB_PATH} -name '*.json'").strip().split('\n')
        for fpath in files:
            if not fpath: continue
            try:
                content = client.succeed(f"cat {fpath}")
                data = json.loads(content)
                if isinstance(data, dict) and "jobs" in data:
                    jobs = data["jobs"]
                    for jid, jval in jobs.items():
                        if "consumer" in jval.get("name", ""):
                            return [jid]

                    if jobs:
                         return [list(jobs.keys())[0]]
            except:
                pass
        return []

    subset_jobs = get_subset_jobs()
    if not subset_jobs:
        raise Exception("Could not find any jobs to run!")

    run_jobs_arg = " ".join(subset_jobs)
    print(f"Selected subset of jobs to run: {run_jobs_arg}")


    def run_remote_test(runtime, mount_mode="pure"):
        print(f"\n>>> Testing Remote: runtime={runtime}, mount={mount_mode} <<<")

        mount_config = ""
        if mount_mode == "impure":
            mount_config = "mount_host_paths = true"
        elif mount_mode == "specific":
            mount_config = 'mount_paths = ["/tmp/host-data"]'

        config = f"""
    submission_target = "remote"
    [targets.local]
    base_path = "/root/repx-local"
    [targets.remote]
    address = "repxuser@target"
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "{runtime}"
    {mount_config}
    [targets.remote.local]
    execution_types = ["{runtime}"]
    local_concurrency = 2
    """

        client.succeed("mkdir -p /root/.config/repx")
        client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

        client.succeed(f"repx run {run_jobs_arg} --lab {LAB_PATH}")

        rc, _ = target.execute(f"find {base_path}/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("!!! TEST FAILED - No SUCCESS markers found")
            print("Output tree:")
            print(target.succeed(f"find {base_path}/outputs -maxdepth 4"))
            print("Stderr logs:")
            print(target.succeed(f"find {base_path}/outputs -name 'stderr.log' -exec echo '--- {{}} ---' \\; -exec cat {{}} \\;"))
            raise Exception(f"Remote test failed: runtime={runtime}, mount={mount_mode}")

        print(f"✓ Remote test PASSED: runtime={runtime}, mount={mount_mode}")

        target.succeed(f"rm -rf {base_path}/outputs/*")
        target.succeed(f"rm -rf {base_path}/cache/images-loaded 2>/dev/null || true")

    with subtest("Remote Bwrap - Pure"):
        run_remote_test("bwrap", "pure")

    with subtest("Remote Bwrap - Impure"):
        run_remote_test("bwrap", "impure")

    with subtest("Remote Bwrap - Specific Mounts"):
        run_remote_test("bwrap", "specific")

    target.wait_for_unit("docker.service")

    with subtest("Remote Docker - Pure"):
        run_remote_test("docker", "pure")

    with subtest("Remote Docker - Impure"):
        run_remote_test("docker", "impure")

    with subtest("Remote Docker - Specific Mounts"):
        run_remote_test("docker", "specific")

    with subtest("Remote Podman - Pure"):
        run_remote_test("podman", "pure")

    with subtest("Remote Podman - Impure"):
        run_remote_test("podman", "impure")

    with subtest("Remote Podman - Specific Mounts"):
        run_remote_test("podman", "specific")

    print("\n" + "=" * 60)
    print("ALL REMOTE TESTS PASSED!")
    print("=" * 60)
  '';
}
