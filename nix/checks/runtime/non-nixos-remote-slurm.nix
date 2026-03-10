{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
  staticBusybox = pkgs.pkgsStatic.busybox;

  nonNixosLoginWrapper = pkgs.writeShellScript "non-nixos-login" ''
    exec ${pkgs.bubblewrap}/bin/bwrap \
      --unshare-all \
      --share-net \
      --bind /host-root / \
      --bind /etc /etc \
      --bind /usr /usr \
      --bind /var /var \
      --bind /tmp /tmp \
      --bind /home /home \
      --bind /run /run \
      --dev-bind /dev /dev \
      --proc /proc \
      -- /bin/sh -lc "$SSH_ORIGINAL_COMMAND"
  '';
in

pkgs.testers.runNixOSTest {
  name = "non-nixos-remote-slurm";

  extraPythonPackages = _: [ getSubsetJobs ];

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
          pkgs.bubblewrap
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
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
        ];

        services = {
          openssh = {
            enable = true;
            extraConfig = ''
              Match User repxuser
                ForceCommand ${nonNixosLoginWrapper}
            '';
          };
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
    import os
    from get_subset_jobs import get_subset_jobs
    start_all()

    cluster.succeed("mkdir -p /host-root")
    for d in ["bin", "etc", "usr", "var", "tmp", "home", "run", "dev", "proc"]:
        cluster.succeed(f"mkdir -p /host-root/{d}")

    cluster.succeed("chown root:root /host-root")
    cluster.succeed("chmod 755 /host-root")

    cluster.succeed("cp ${staticBusybox}/bin/busybox /host-root/bin/busybox")
    for cmd in ["sh", "cat", "mkdir", "echo", "find", "grep", "ls", "rm", "cp", "chmod", "pwd", "env", "test", "true", "false", "sleep", "wc", "tar", "ln", "uname", "id", "dirname", "basename", "head", "tail", "sort", "xargs", "tr", "sed", "awk", "tee", "touch", "rmdir", "mv", "dd", "df", "du", "chown"]:
        cluster.succeed(f"ln -sf busybox /host-root/bin/{cmd}")

    cluster.succeed("mkdir -p /home/repxuser")
    cluster.succeed("echo 'export PATH=/bin:/usr/local/bin:/usr/bin' > /home/repxuser/.profile")
    cluster.succeed("chown repxuser:users /home/repxuser/.profile")

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
    client.succeed("ssh repxuser@cluster 'test ! -d /nix'")
    print("Verified: /nix does not exist for repxuser via SSH")

    cluster.wait_for_unit("munged.service")
    cluster.wait_for_unit("slurmctld.service")
    cluster.wait_for_unit("slurmd.service")
    cluster.succeed("sinfo")

    base_path = "/home/repxuser/repx-store"
    cluster.succeed(f"mkdir -p {base_path}")
    cluster.succeed(f"chown -R repxuser:users {base_path}")

    LAB_PATH = "${referenceLab}"

    subset_jobs = get_subset_jobs(LAB_PATH)
    if not subset_jobs:
        print(f"ERROR: Could not find any jobs in {LAB_PATH}.")
        os.system(f"find {LAB_PATH} -maxdepth 4")
        raise Exception("get_subset_jobs returned empty list for non-nixos-remote-slurm")

    run_args = " ".join(subset_jobs)
    print(f"Running subset: {run_args}")

    config = f"""
    submission_target = "cluster"
    [targets.local]
    base_path = "/root/repx-local"

    [targets.cluster]
    address = "repxuser@cluster"
    base_path = "{base_path}"
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

    print("--- Testing non-nixos-remote-slurm with Reference Lab ---")

    client.succeed(f"repx run {run_args} --lab ${referenceLab}")

    print("Waiting for slurm jobs to finish...")
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

    success_count = int(cluster.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"Found {success_count} SUCCESS markers")

    if success_count == 0:
        print("!!! TEST FAILED. Dumping debug info:")
        print("\n>>> SLURM JOB HISTORY (sacct):")
        print(cluster.succeed("sacct --format=JobID,JobName,State,ExitCode"))
        print("\n>>> OUTPUT DIRECTORY TREE:")
        print(cluster.succeed(f"find {base_path}/outputs -maxdepth 4"))
        print("\n>>> SLURM OUTPUT LOGS:")
        print(cluster.succeed(f"find {base_path}/outputs -name 'slurm-*.out' -exec echo '--- {{}} ---' \\; -exec cat {{}} \\;"))
        print("\n>>> STDERR LOGS:")
        print(cluster.succeed(f"find {base_path}/outputs -name 'stderr.log' -exec echo '--- {{}} ---' \\; -exec cat {{}} \\;"))
        raise Exception("No SUCCESS markers found! non-nixos-remote-slurm failed.")

    print("\n" + "=" * 60)
    print("NON-NIXOS REMOTE SLURM TEST COMPLETED")
    print("=" * 60)
  '';
}
