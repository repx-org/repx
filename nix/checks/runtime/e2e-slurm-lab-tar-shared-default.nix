{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
in

pkgs.testers.runNixOSTest {
  name = "e2e-slurm-lab-tar-shared-default";

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
          repx
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
    from get_subset_jobs import get_subset_jobs
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

    subset_jobs = get_subset_jobs(LAB_PATH, prefer_resource_hints=True)
    if not subset_jobs:
        raise Exception("Failed to find subset of jobs")
    run_args = " ".join(subset_jobs)
    print(f"Running subset: {run_args}")

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

    with subtest("Submit jobs WITHOUT --artifact-store (default = shared)"):
        print("--- Submitting jobs with default artifact store ---")
        client.succeed(f"repx run {run_args} --lab ${referenceLab}")

    with subtest("Wait for slurm jobs to complete"):
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

    with subtest("Jobs succeeded"):
        rc, _ = cluster.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("\n>>> SLURM JOB HISTORY (sacct):")
            print(cluster.succeed("sacct --format=JobID,JobName,State,ExitCode"))
            print("\n>>> OUTPUT TREE:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -maxdepth 4"))
            print("\n>>> SLURM LOGS:")
            print(cluster.succeed("find /home/repxuser/repx-store -name 'slurm-*.out' -exec echo '--- {} ---' \\; -exec cat {} \\;"))
            raise Exception("Jobs failed!")

        success_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Found {success_count} SUCCESS markers")

    with subtest("NO lab-tars directory created"):
        rc, output = cluster.execute(
            "find /home/repxuser/repx-store/lab-tars -name '*.tar' 2>/dev/null | wc -l"
        )
        lab_tar_count = int(output.strip()) if rc == 0 else 0
        print(f"Lab tars on shared: {lab_tar_count}")
        if lab_tar_count > 0:
            raise Exception(
                f"Found {lab_tar_count} lab tar(s) on shared storage! "
                "Default artifact-store=shared should not create tars."
            )

    with subtest("NO extraction markers on node-local"):
        rc, output = cluster.execute(
            "find /local/scratch/repx -name '.extracted-*' 2>/dev/null | wc -l"
        )
        marker_count = int(output.strip()) if rc == 0 else 0
        rc2, output2 = cluster.execute(
            "find /local/scratch/repx/labs 2>/dev/null | wc -l"
        )
        lab_dirs = output2.strip() if rc2 == 0 else "0"
        print(f"Extraction markers on node-local: {marker_count}")
        print(f"Lab dirs on node-local: {lab_dirs}")
        if marker_count > 0:
            raise Exception("Found extraction markers on node-local with default shared mode!")

    with subtest("NO flock/tar bootstrap in sbatch scripts"):
        sbatch_content = cluster.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec cat {} \\; 2>/dev/null"
        )
        if "flock" in sbatch_content and "tar xf" in sbatch_content:
            raise Exception(
                "Sbatch scripts contain flock+tar bootstrap, but artifact-store is shared!"
            )
        if "--local-artifacts-path" in sbatch_content:
            raise Exception(
                "Sbatch scripts contain --local-artifacts-path, but artifact-store is shared!"
            )
        print("Confirmed: no lab-tar bootstrap in sbatch scripts with default mode")

    with subtest("Image cache uses node-local (node_local_path still works for cache)"):
        local_cache = int(cluster.succeed(
            "find /local/scratch/repx/cache -name 'SUCCESS' 2>/dev/null | wc -l"
        ).strip())
        print(f"Image cache SUCCESS markers on node-local: {local_cache}")
        print(f"(node_local_path is still used for image caching: {local_cache > 0})")

    print("\n" + "=" * 60)
    print("E2E SLURM LAB-TAR SHARED DEFAULT REGRESSION TEST COMPLETED")
    print("=" * 60)
  '';
}
