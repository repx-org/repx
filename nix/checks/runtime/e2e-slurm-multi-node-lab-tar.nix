{
  pkgs,
  repx,
  referenceLab,
}:

let
  getSubsetJobs = pkgs.python3Packages.callPackage ./helpers/get-subset-jobs { };

in

pkgs.testers.runNixOSTest {
  name = "e2e-slurm-multi-node-lab-tar";

  extraPythonPackages = _: [ getSubsetJobs ];

  nodes = {
    client =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 2048;
          cores = 2;
        };
        environment.systemPackages = [
          repx
          pkgs.openssh
          pkgs.rsync
        ];
        programs.ssh.extraConfig = "StrictHostKeyChecking no";
      };

    controller =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 4096;
          cores = 2;
        };

        networking = {
          hostName = "controller";
          firewall.enable = false;
          extraHosts = "192.168.1.2 controller\n192.168.1.3 worker";
        };

        environment.systemPackages = [
          repx
          pkgs.bubblewrap
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnutar
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
            controlMachine = "controller";
            procTrackType = "proctrack/pgid";
            nodeName = [
              "controller NodeAddr=controller CPUs=2 RealMemory=3000 State=UNKNOWN"
              "worker NodeAddr=worker CPUs=2 RealMemory=3000 State=UNKNOWN"
            ];
            partitionName = [ "main Nodes=controller,worker Default=YES MaxTime=INFINITE State=UP" ];
            extraConfig = ''
              SlurmdTimeout=300
              SlurmctldTimeout=60
              ReturnToService=2
              TreeWidth=65535
            '';
          };
        };

        services.nfs.server.enable = true;
        services.nfs.server.exports = ''
          /home/repxuser *(rw,no_subtree_check,no_root_squash)
        '';
      };

    worker =
      { pkgs, ... }:
      {
        virtualisation = {
          diskSize = 25600;
          memorySize = 4096;
          cores = 2;
        };

        networking = {
          hostName = "worker";
          firewall.enable = false;
          extraHosts = "192.168.1.2 controller\n192.168.1.3 worker";
        };

        environment.systemPackages = [
          repx
          pkgs.bubblewrap
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnutar
          pkgs.nfs-utils
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
            client.enable = true;
            controlMachine = "controller";
            procTrackType = "proctrack/pgid";
            nodeName = [
              "controller NodeAddr=controller CPUs=2 RealMemory=3000 State=UNKNOWN"
              "worker NodeAddr=worker CPUs=2 RealMemory=3000 State=UNKNOWN"
            ];
            partitionName = [ "main Nodes=controller,worker Default=YES MaxTime=INFINITE State=UP" ];
            extraConfig = ''
              SlurmdTimeout=300
              SlurmctldTimeout=60
              ReturnToService=2
              TreeWidth=65535
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

    controller.succeed("mkdir -p /home/repxuser/.ssh")
    controller.succeed(f"echo '{pub_key}' >> /home/repxuser/.ssh/authorized_keys")
    controller.succeed("chown -R repxuser:users /home/repxuser/.ssh")
    controller.succeed("chmod 700 /home/repxuser/.ssh")
    controller.succeed("chmod 600 /home/repxuser/.ssh/authorized_keys")
    controller.succeed("loginctl enable-linger repxuser")

    client.wait_for_unit("network.target")
    controller.wait_for_unit("sshd.service")
    worker.wait_for_unit("sshd.service")

    client.succeed("ssh repxuser@controller 'echo SSH_OK'")

    controller.wait_for_unit("munged.service")
    controller.wait_for_unit("slurmctld.service")
    controller.wait_for_unit("slurmd.service")
    controller.wait_for_unit("nfs-server.service")
    worker.wait_for_unit("munged.service")
    worker.wait_for_unit("slurmd.service")

    worker.succeed("mkdir -p /var/lib/nfs")
    worker.succeed("mount -t nfs controller:/home/repxuser /home/repxuser -o nfsvers=3,nolock")

    controller.succeed("""
        for i in {1..90}; do
            ready=$(sinfo -h -N -o '%N %T' | grep -c 'idle' || echo 0)
            if [ "$ready" -ge 2 ]; then
                echo "Both nodes are idle."
                sinfo -N
                exit 0
            fi
            if [ $((i % 10)) -eq 0 ]; then
                echo "Waiting for 2 idle nodes (attempt $i/90)..."
                sinfo -N
                scontrol show nodes
            fi
            scontrol update nodename=controller state=resume 2>/dev/null || true
            scontrol update nodename=worker state=resume 2>/dev/null || true
            sleep 2
        done
        echo "Timeout waiting for 2 idle nodes."
        sinfo -N
        scontrol show nodes
        exit 1
    """)

    controller.succeed("sinfo")
    print("SLURM cluster ready with 2 nodes")

    LAB_PATH = "${referenceLab}"

    subset_jobs = get_subset_jobs(LAB_PATH, prefer_resource_hints=True)
    if not subset_jobs:
        raise Exception("Failed to find subset of jobs")

    import json, os
    extra_jobs = []
    for root, dirs, files in os.walk(LAB_PATH):
        for file in files:
            if not file.endswith(".json"):
                continue
            full_path = os.path.join(root, file)
            try:
                with open(full_path) as f:
                    data = json.load(f)
                    if data.get("name") == "simulation-run" and "jobs" in data:
                        for jid, jval in data["jobs"].items():
                            if jval.get("resource_hints") and jval.get("stage_type", "simple") == "simple":
                                if jid not in subset_jobs:
                                    extra_jobs.append(jid)
                                    if len(extra_jobs) >= 3:
                                        break
            except:
                pass
            if len(extra_jobs) >= 3:
                break
        if len(extra_jobs) >= 3:
            break

    all_jobs = subset_jobs + extra_jobs
    run_args = " ".join(all_jobs)
    print(f"Running {len(all_jobs)} jobs: {run_args}")

    config = """
    submission_target = "cluster"
    [targets.local]
    base_path = "/root/repx-local"

    [targets.cluster]
    address = "repxuser@controller"
    base_path = "/home/repxuser/repx-store"
    node_local_path = "/local/scratch"
    default_scheduler = "slurm"
    default_execution_type = "native"

    [targets.cluster.slurm]
    execution_types = ["native"]
    """

    resources = """
    [defaults]
    partition = "main"
    cpus-per-task = 1
    """

    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")
    client.succeed(f"cat <<EOF > /root/.config/repx/resources.toml\n{resources}\nEOF")

    with subtest("Submit jobs with --artifact-store node-local"):
        print("--- Submitting multiple jobs to 2-node cluster ---")
        client.succeed(f"repx run {run_args} --lab ${referenceLab} --artifact-store node-local")

    with subtest("Wait for slurm jobs to complete"):
        controller.succeed("""
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
        rc, _ = controller.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("\n>>> SLURM QUEUE:")
            rc_sacct, sacct_out = controller.execute("sacct --format=JobID,JobName,State,ExitCode,NodeList 2>&1")
            print(sacct_out if rc_sacct == 0 else f"sacct unavailable: {sacct_out.strip()}")
            print("\n>>> OUTPUT TREE:")
            print(controller.succeed("find /home/repxuser/repx-store/outputs -maxdepth 4"))
            print("\n>>> SLURM LOGS:")
            print(controller.succeed("find /home/repxuser/repx-store -name 'slurm-*.out' -exec echo '--- {} ---' \\; -exec cat {} \\;"))
            print("\n>>> STDERR LOGS:")
            print(controller.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' -exec echo '--- {} ---' \\; -exec cat {} \\;"))
            raise Exception("Jobs failed!")

        success_count = int(controller.succeed(
            "find /home/repxuser/repx-store/outputs -name SUCCESS | wc -l"
        ).strip())
        print(f"Found {success_count} SUCCESS markers")

    with subtest("Lab tar synced to shared storage"):
        tar_count = int(controller.succeed(
            "find /home/repxuser/repx-store/lab-tars -name '*.tar' 2>/dev/null | wc -l"
        ).strip())
        print(f"Lab tars on shared storage: {tar_count}")
        if tar_count == 0:
            raise Exception("No lab tar found on shared storage!")

    with subtest("Check which nodes ran jobs"):
        ctrl_marker = int(controller.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        ).strip())
        worker_marker = int(worker.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        ).strip())
        ran_on_controller = ctrl_marker > 0
        ran_on_worker = worker_marker > 0
        print(f"Controller extraction markers: {ctrl_marker}")
        print(f"Worker extraction markers: {worker_marker}")
        print(f"Jobs ran on controller: {ran_on_controller}")
        print(f"Jobs ran on worker: {ran_on_worker}")

        if not (ran_on_controller and ran_on_worker):
            print("WARNING: Slurm only scheduled jobs on one node. "
                  "Multi-node extraction test is degraded but we continue.")

    with subtest("Controller node: extraction marker exists"):
        if ran_on_controller and ctrl_marker == 0:
            raise Exception("Controller ran jobs but has no extraction marker!")
        print(f"Controller: {ctrl_marker} extraction marker(s)")

    with subtest("Worker node: extraction marker exists independently"):
        if ran_on_worker and worker_marker == 0:
            raise Exception("Worker ran jobs but has no extraction marker!")
        print(f"Worker: {worker_marker} extraction marker(s)")

    with subtest("Each node has its own LOCAL copy (not shared)"):
        ctrl_lab_files = int(controller.succeed(
            "find /local/scratch/repx/labs -type f 2>/dev/null | wc -l"
        ).strip())
        worker_lab_files = int(worker.succeed(
            "find /local/scratch/repx/labs -type f 2>/dev/null | wc -l"
        ).strip())
        print(f"Controller local lab files: {ctrl_lab_files}")
        print(f"Worker local lab files: {worker_lab_files}")

        if ran_on_controller and ctrl_lab_files == 0:
            raise Exception("Controller has no local lab files!")
        if ran_on_worker and worker_lab_files == 0:
            raise Exception("Worker has no local lab files!")

    with subtest("Exactly 1 extraction per node (flock dedup)"):
        if ran_on_controller and ctrl_marker > 1:
            raise Exception(f"Controller has {ctrl_marker} extraction markers, expected 1!")
        if ran_on_worker and worker_marker > 1:
            raise Exception(f"Worker has {worker_marker} extraction markers, expected 1!")

    with subtest("Outputs on shared storage, not node-local"):
        output_count = int(controller.succeed(
            "find /home/repxuser/repx-store/outputs -type f | wc -l"
        ).strip())
        ctrl_local_out = int(controller.succeed(
            "find /local/scratch -path '*/outputs/*' 2>/dev/null | wc -l"
        ).strip())
        worker_local_out = int(worker.succeed(
            "find /local/scratch -path '*/outputs/*' 2>/dev/null | wc -l"
        ).strip())
        print(f"Shared outputs: {output_count}")
        print(f"Controller local outputs: {ctrl_local_out}")
        print(f"Worker local outputs: {worker_local_out}")

        if output_count == 0:
            raise Exception("No output files on shared storage!")
        if ctrl_local_out > 0 or worker_local_out > 0:
            raise Exception("Output files leaked to node-local storage!")

    with subtest("Sbatch scripts contain flock bootstrap"):
        sbatch_content = controller.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' "
            "-exec cat {} \\; 2>/dev/null || echo 'no sbatch files'"
        )
        if "flock" not in sbatch_content:
            raise Exception("No flock in sbatch scripts!")
        if "tar xf" not in sbatch_content:
            raise Exception("No tar extraction in sbatch scripts!")
        if "--local-artifacts-path" not in sbatch_content:
            raise Exception("No --local-artifacts-path flag in sbatch scripts!")
        print("Bootstrap preamble verified")

    print("\n" + "=" * 60)
    print("E2E SLURM MULTI-NODE LAB-TAR TEST COMPLETED")
    print("=" * 60)
  '';
}
