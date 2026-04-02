{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "e2e-slurm-multi-node-scatter-gather-lab-tar";

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

        networking.hostName = "controller";
        networking.firewall.enable = false;

        environment.systemPackages = [
          repx
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnutar
          pkgs.jq
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
              "controller NodeAddr=controller CPUs=1 RealMemory=3000 State=UNKNOWN"
              "worker NodeAddr=worker CPUs=1 RealMemory=3000 State=UNKNOWN"
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

        networking.hostName = "worker";
        networking.firewall.enable = false;

        environment.systemPackages = [
          repx
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnutar
          pkgs.jq
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
              "controller NodeAddr=controller CPUs=1 RealMemory=3000 State=UNKNOWN"
              "worker NodeAddr=worker CPUs=1 RealMemory=3000 State=UNKNOWN"
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
    import json
    import os
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
            scontrol update nodename=controller state=resume 2>/dev/null || true
            scontrol update nodename=worker state=resume 2>/dev/null || true
            sleep 2
        done
        echo "Timeout waiting for 2 idle nodes."
        sinfo -N
        exit 1
    """)
    print("SLURM cluster ready with 2 nodes")

    LAB_PATH = "${referenceLab}"

    def find_scatter_gather_job():
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
                                if jval.get("stage_type") == "scatter-gather":
                                    return jid
                except:
                    pass
        return None

    sg_job_id = find_scatter_gather_job()
    if not sg_job_id:
        raise Exception("No scatter-gather job found in reference lab!")
    print(f"Scatter-gather job ID: {sg_job_id}")

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
    """

    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")
    client.succeed(f"cat <<EOF > /root/.config/repx/resources.toml\n{resources}\nEOF")

    with subtest("Submit scatter-gather job with --artifact-store node-local"):
        print(f"--- Submitting SG job {sg_job_id} to 2-node cluster ---")
        client.succeed(f"repx run {sg_job_id} --lab ${referenceLab} --artifact-store node-local")

    with subtest("Wait for slurm jobs to complete"):
        controller.succeed("""
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

    with subtest("Scatter-gather job succeeded"):
        rc, _ = controller.execute("find /home/repxuser/repx-store/outputs -name SUCCESS | grep .")
        if rc != 0:
            print("\n>>> SLURM QUEUE:")
            rc2, sacct_out = controller.execute("sacct --format=JobID,JobName,State,ExitCode,NodeList --noheader -u repxuser 2>&1")
            print(sacct_out if rc2 == 0 else f"sacct unavailable: {sacct_out.strip()}")
            print("\n>>> OUTPUT TREE:")
            print(controller.succeed("find /home/repxuser/repx-store/outputs -maxdepth 5 2>/dev/null || echo 'no outputs dir'"))
            print("\n>>> SBATCH SCRIPTS:")
            print(controller.succeed("find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec echo '=== {} ===' \\; -exec head -40 {} \\; 2>/dev/null || echo 'no sbatch'"))
            print("\n>>> SLURM LOGS (first 2):")
            print(controller.succeed("find /home/repxuser/repx-store -name 'slurm-*.out' | head -2 | xargs -I{} sh -c 'echo \"--- {} ---\" && cat {}' 2>/dev/null || echo 'no slurm logs'"))
            print("\n>>> STDERR LOGS (first 2):")
            print(controller.succeed("find /home/repxuser/repx-store/outputs -name 'stderr.log' | head -2 | xargs -I{} sh -c 'echo \"--- {} ---\" && cat {}' 2>/dev/null || echo 'no stderr'"))
            raise Exception("Scatter-gather job failed!")

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

    with subtest("Both nodes extracted the tar independently"):
        ctrl_marker = int(controller.succeed(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        ).strip())
        rc_w, worker_marker_out = worker.execute(
            "find /local/scratch/repx/labs -name '.extracted-*' 2>/dev/null | wc -l"
        )
        worker_marker = int(worker_marker_out.strip()) if rc_w == 0 else 0
        print(f"Controller extraction markers: {ctrl_marker}")
        print(f"Worker extraction markers: {worker_marker}")

        if ctrl_marker == 0:
            raise Exception("Controller has no extraction marker — no workers ran there?")
        if worker_marker == 0:
            raise Exception("Worker has no extraction marker — no workers ran there?")

    with subtest("Each node has its own local lab files"):
        ctrl_lab_files = int(controller.succeed(
            "find /local/scratch/repx/labs -type f 2>/dev/null | wc -l"
        ).strip())
        rc_wf, worker_lab_out = worker.execute(
            "find /local/scratch/repx/labs -type f 2>/dev/null | wc -l"
        )
        worker_lab_files = int(worker_lab_out.strip()) if rc_wf == 0 else 0
        print(f"Controller local lab files: {ctrl_lab_files}")
        print(f"Worker local lab files: {worker_lab_files}")
        if ctrl_lab_files == 0:
            raise Exception("Controller has no local lab files!")
        if worker_lab_files == 0:
            raise Exception("Worker has no local lab files!")

    with subtest("Exactly 1 extraction per node (flock dedup despite many workers)"):
        if ctrl_marker != 1:
            raise Exception(f"Controller has {ctrl_marker} extraction markers, expected exactly 1!")
        if worker_marker != 1:
            raise Exception(f"Worker has {worker_marker} extraction markers, expected exactly 1!")
        print("Flock dedup confirmed: 1 marker per node")

    with subtest("Outputs on shared NFS, not node-local"):
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

    with subtest("Orchestrator sbatch passes --lab-tar-path"):
        sbatch_content = controller.succeed(
            "find /home/repxuser/repx-store/submissions -name '*.sbatch' -exec cat {} \\; 2>/dev/null"
        )
        if "--lab-tar-path" not in sbatch_content:
            raise Exception("Orchestrator sbatch does not contain --lab-tar-path!")
        if "flock" not in sbatch_content:
            raise Exception("Sbatch scripts missing flock bootstrap!")
        print("Bootstrap preamble and --lab-tar-path verified")

    with subtest("Worker SLURM IDs manifest exists (SG produced workers)"):
        manifest = controller.succeed(
            "find /home/repxuser/repx-store/outputs -name 'worker_slurm_ids.json' -exec cat {} \\; 2>/dev/null"
        ).strip()
        if manifest:
            worker_ids = json.loads(manifest)
            print(f"Worker SLURM IDs: {worker_ids}")
            if len(worker_ids) < 2:
                print(f"WARNING: Only {len(worker_ids)} worker(s), may not have hit both nodes")
            else:
                print(f"Scatter-gather spawned {len(worker_ids)} workers across 2-node cluster")
        else:
            print("Warning: No worker_slurm_ids.json found")

    print("\n" + "=" * 70)
    print("E2E SLURM MULTI-NODE SCATTER-GATHER LAB-TAR TEST COMPLETED")
    print("=" * 70)
  '';
}
