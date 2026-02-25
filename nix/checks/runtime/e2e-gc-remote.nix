{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-gc-remote-test";

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
        };

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
          password = "password";
          home = "/home/repxuser";
          createHome = true;
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
    server.succeed("mkdir -p /home/repxuser/.ssh")
    server.succeed(f"echo '{pub_key}' >> /home/repxuser/.ssh/authorized_keys")
    server.succeed("chown -R repxuser:users /home/repxuser/.ssh")
    server.succeed("chmod 700 /home/repxuser/.ssh")
    server.succeed("chmod 600 /home/repxuser/.ssh/authorized_keys")

    client.wait_for_unit("network.target")
    server.wait_for_unit("sshd.service")
    client.succeed("ssh repxuser@server 'echo SSH_OK'")

    base_path = "/home/repxuser/repx-store"

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
                                        return [jid]
                                if jobs:
                                    return [list(jobs.keys())[0]]
                    except Exception as e:
                        print(f"Warning: Failed to read {full_path}: {e}")
        return []

    subset_jobs = get_subset_jobs()
    if not subset_jobs:
        raise Exception("Failed to find subset of jobs.")

    run_args = " ".join(subset_jobs)
    print(f"Running subset of jobs: {run_args}")

    config = f"""
    submission_target = "remote"
    [targets.local]
    base_path = "/root/repx-local"
    [targets.remote]
    address = "repxuser@server"
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "bwrap"
    [targets.remote.local]
    execution_types = ["bwrap"]
    local_concurrency = 2
    """
    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    with subtest("Run job over SSH to populate remote store"):
        print("--- Running job on remote target ---")
        client.succeed(f"repx run {run_args} --lab ${referenceLab}")

        rc, _ = server.execute(f"find {base_path}/outputs -name SUCCESS | grep .")
        if rc != 0:
            print(server.succeed(f"find {base_path}/outputs -maxdepth 4"))
            raise Exception("Run failed on remote target")
        print("Remote store populated successfully.")

    with subtest("GC list on remote target shows auto roots"):
        print("--- Testing repx gc list --target remote ---")
        output = client.succeed("repx gc list --target remote --lab ${referenceLab}")
        print(f"gc list output:\n{output}")

        assert "auto" in output, \
            f"Expected 'auto' root from the remote run, got:\n{output}"
        print("GC list on remote target shows auto roots correctly.")

    with subtest("GC pin on remote target"):
        print("--- Testing repx gc pin --target remote ---")
        client.succeed("repx gc pin --target remote --name remote-pin --lab ${referenceLab}")

        output = client.succeed("repx gc list --target remote --lab ${referenceLab}")
        print(f"gc list after pin:\n{output}")

        assert "pinned" in output, \
            f"Expected 'pinned' root after pin, got:\n{output}"
        assert "remote-pin" in output, \
            f"Expected 'remote-pin' in list, got:\n{output}"

        server.succeed(f"test -L {base_path}/gcroots/pinned/remote-pin")
        print("GC pin created symlink on remote server correctly.")

    with subtest("Pinned root survives GC on remote target"):
        print("--- Testing GC collection preserves remote pinned root ---")

        server.succeed(f"su - repxuser -c 'mkdir -p {base_path}/artifacts/dead-remote-artifact'")
        server.succeed(f"su - repxuser -c \"echo dead > {base_path}/artifacts/dead-remote-artifact/f.txt\"")

        client.succeed("repx gc --target remote --lab ${referenceLab}")

        server.succeed(f"test -L {base_path}/gcroots/pinned/remote-pin")

        rc, _ = server.execute(f"test -d {base_path}/artifacts/dead-remote-artifact")
        assert rc != 0, "Dead artifact should have been collected on remote"

        print("GC preserved pinned root and removed dead artifact on remote.")

    with subtest("GC unpin on remote target"):
        print("--- Testing repx gc unpin --target remote ---")
        client.succeed("repx gc unpin remote-pin --target remote --lab ${referenceLab}")

        output = client.succeed("repx gc list --target remote --lab ${referenceLab}")
        print(f"gc list after unpin:\n{output}")

        assert "remote-pin" not in output, \
            f"'remote-pin' should be gone after unpin, got:\n{output}"

        rc, _ = server.execute(f"test -L {base_path}/gcroots/pinned/remote-pin")
        assert rc != 0, "Pinned symlink should be removed on remote server"
        print("GC unpin removed symlink on remote server correctly.")

    with subtest("GC unpin nonexistent name fails on remote"):
        print("--- Testing repx gc unpin with bad name on remote ---")
        rc, output = client.execute("repx gc unpin does-not-exist --target remote --lab ${referenceLab}")
        assert rc != 0, \
            f"Unpin of nonexistent name should fail on remote, but got rc={rc}"
        print("GC unpin correctly rejects nonexistent name on remote.")

    print("\n" + "=" * 60)
    print("E2E GC REMOTE TEST COMPLETED")
    print("=" * 60)
  '';
}
