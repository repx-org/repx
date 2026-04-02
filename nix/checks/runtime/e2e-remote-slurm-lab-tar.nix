{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };
in

pkgs.testers.runNixOSTest {
  name = "e2e-remote-slurm-lab-tar";

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

    with subtest("Submit jobs with --lab-tar"):
        print("--- Submitting jobs with --lab-tar ---")
        client.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")

    with subtest("Wait for slurm jobs to complete"):
        cluster.succeed("""
            for i in {1..600}; do
                total=$(squeue -h -u repxuser | wc -l)
                running=$(squeue -h -u repxuser -t RUNNING | wc -l)

                if [ "$total" -eq 0 ]; then
                    echo "Queue empty, jobs finished."
                    exit 0
                fi

                if [ "$running" -eq 0 ] && [ "$total" -gt 0 ]; then
                    sleep 5
                    running2=$(squeue -h -u repxuser -t RUNNING | wc -l)
                    if [ "$running2" -eq 0 ]; then
                        echo "No running jobs. Cancelling stale held jobs."
                        squeue -h -u repxuser -o '%i' | xargs -r scancel 2>/dev/null || true
                        exit 0
                    fi
                fi

                sleep 2
            done
            echo "Timeout waiting for Slurm jobs to finish."
            exit 1
        """)

    with subtest("Jobs succeeded"):
        rc, _ = cluster.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("\n>>> SLURM JOB HISTORY:")
            rc2, sacct_out = cluster.execute("sacct --format=JobID,JobName,State,ExitCode 2>&1")
            print(sacct_out if rc2 == 0 else f"sacct unavailable: {sacct_out.strip()}")
            print("\n>>> OUTPUT DIRECTORY TREE:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -maxdepth 4"))
            print("\n>>> SLURM OUTPUT LOGS:")
            print(cluster.succeed("find /home/repxuser/repx-store -name 'slurm-*.out' -exec echo '--- {} ---' \\; -exec cat {} \\;"))
            print("\n>>> REPX STDERR LOGS:")
            print(cluster.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' -exec echo '--- {} ---' \\; -exec cat {} \\;"))
            raise Exception("Jobs failed!")

        success_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Found {success_count} SUCCESS markers")

    with subtest("Lab tar synced to shared storage"):
        tar_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/lab-tars -name '*.tar' 2>/dev/null | wc -l"
        ).strip())
        print(f"Lab tars on shared storage: {tar_count}")
        if tar_count == 0:
            raise Exception("No lab tar found on shared storage!")

    with subtest("Lab extracted to node-local"):
        lab_count = int(cluster.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Extraction markers on node-local: {lab_count}")
        if lab_count == 0:
            raise Exception("No extraction marker on node-local!")

    with subtest("Bootstrap preamble in sbatch scripts"):
        sbatch_content = cluster.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' "
            "-exec cat {} \\; 2>/dev/null || echo 'no sbatch files'"
        )
        print(f"Sbatch content length: {len(sbatch_content)} chars")

        if "flock" not in sbatch_content:
            print(f"SBATCH CONTENT:\n{sbatch_content[:2000]}")
            raise Exception("No flock in sbatch scripts!")
        if "tar xf" not in sbatch_content:
            raise Exception("No tar extraction in sbatch scripts!")
        if "--local-artifacts-path" not in sbatch_content:
            raise Exception("No --local-artifacts-path in sbatch scripts!")
        print("Bootstrap preamble verified in sbatch scripts")

    with subtest("Outputs on shared storage, not node-local"):
        output_count = int(cluster.succeed(
            "find /home/repxuser/repx-store/outputs -type f | wc -l"
        ).strip())
        local_output = int(cluster.succeed(
            "find /local/scratch -path '*/outputs/*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Files in shared outputs: {output_count}")
        print(f"Files in node-local outputs: {local_output}")
        if output_count == 0:
            raise Exception("No output files found on shared storage!")
        if local_output > 0:
            raise Exception("Output files leaked to node-local storage!")

    print("\n" + "=" * 60)
    print("E2E REMOTE SLURM LAB-TAR TEST COMPLETED")
    print("=" * 60)
  '';
}
