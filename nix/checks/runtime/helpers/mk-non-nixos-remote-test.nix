{
  pkgs,
  repx,
  referenceLab,
  testName,
  runtime,
  mountMode,
  useSubset ? false,
  runName ? "simulation-run",
  extraValidation ? "",
  bannerText ? "E2E TEST COMPLETED",
}:

let
  isDocker = runtime == "docker";
  isPodman = runtime == "podman";

  staticBusybox = pkgs.pkgsStatic.busybox;

  mountConfig =
    if mountMode == "impure" then
      "mount_host_paths = true"
    else if mountMode == "mount-paths" then
      ''mount_paths = ["/tmp/host-data"]''
    else
      "";

  waitForService = if isDocker then ''target.wait_for_unit("docker.service")'' else "";

  getSubsetJobs = if useSubset then pkgs.python3Packages.callPackage ./get-subset-jobs { } else null;

  subsetImport = pkgs.lib.optionalString useSubset "from get_subset_jobs import get_subset_jobs";

  runCmd =
    if useSubset then
      ''
        subset_jobs = get_subset_jobs("${referenceLab}")
        if not subset_jobs:
            raise Exception("get_subset_jobs returned empty list for ${testName}")
        run_jobs_arg = " ".join(subset_jobs)
        print(f"Running subset: {run_jobs_arg}")
        client.succeed(f"repx run {run_jobs_arg} --lab {LAB_PATH}")
      ''
    else
      ''
        client.succeed(f"repx run ${runName} --lab {LAB_PATH}")
      '';

  mountPathsSetup =
    if mountMode == "mount-paths" then
      ''
        target.succeed("mkdir -p /tmp/host-data")
        target.succeed("echo 'HOST_SECRET_DATA' > /tmp/host-data/secret.txt")
        target.succeed("chmod -R a+r /tmp/host-data")
      ''
    else
      "";

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
  name = testName;

  extraPythonPackages = _: pkgs.lib.optional useSubset getSubsetJobs;

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
          docker.enable = isDocker;
          podman.enable = isPodman;
        };

        networking.dhcpcd.denyInterfaces = [
          "veth*"
        ]
        ++ (if isDocker then [ "docker*" ] else [ ])
        ++ (if isPodman then [ "podman*" ] else [ ]);

        services.openssh = {
          enable = true;
          settings.MaxStartups = "100:30:500";
          extraConfig = ''
            Match User repxuser
              ForceCommand ${nonNixosLoginWrapper}
          '';
        };

        environment.systemPackages = [
          pkgs.bubblewrap
          pkgs.bash
        ]
        ++ (if isDocker then [ pkgs.docker ] else [ ])
        ++ (if isPodman then [ pkgs.podman ] else [ ]);

        users.users.repxuser = {
          isNormalUser = true;
          extraGroups = (if isDocker then [ "docker" ] else [ ]) ++ (if isPodman then [ "podman" ] else [ ]);
          password = "password";
          home = "/home/repxuser";
          createHome = true;
        };
      };
  };

  testScript = ''
    ${subsetImport}
    start_all()

    target.succeed("mkdir -p /host-root")
    for d in ["bin", "etc", "usr", "var", "tmp", "home", "run", "dev", "proc"]:
        target.succeed(f"mkdir -p /host-root/{d}")

    target.succeed("chown root:root /host-root")
    target.succeed("chmod 755 /host-root")

    target.succeed("cp ${staticBusybox}/bin/busybox /host-root/bin/busybox")
    for cmd in ["sh", "cat", "mkdir", "echo", "find", "grep", "ls", "rm", "cp", "chmod", "pwd", "env", "test", "true", "false", "sleep", "wc", "tar", "ln", "uname", "id", "dirname", "basename", "head", "tail", "sort", "xargs", "tr", "sed", "awk", "tee", "touch", "rmdir", "mv", "dd", "df", "du", "chown"]:
        target.succeed(f"ln -sf busybox /host-root/bin/{cmd}")

    ${pkgs.lib.optionalString isDocker ''
      target.succeed("mkdir -p /host-root/usr/local/bin")
      target.succeed("cp $(which docker) /host-root/usr/local/bin/docker")
      target.succeed("chmod +x /host-root/usr/local/bin/docker")
    ''}
    ${pkgs.lib.optionalString isPodman ''
      target.succeed("mkdir -p /host-root/usr/local/bin")
      target.succeed("cp $(which podman) /host-root/usr/local/bin/podman")
      target.succeed("chmod +x /host-root/usr/local/bin/podman")
    ''}

    target.succeed("mkdir -p /home/repxuser")
    target.succeed("echo 'export PATH=/bin:/usr/local/bin:/usr/bin' > /home/repxuser/.profile")
    target.succeed("chown repxuser:users /home/repxuser/.profile")

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
    client.succeed("ssh repxuser@target 'test ! -d /nix'")
    print("Verified: /nix does not exist for repxuser via SSH")

    base_path = "/home/repxuser/repx-store"
    target.succeed(f"mkdir -p {base_path}")
    target.succeed(f"chown -R repxuser:users {base_path}")

    ${mountPathsSetup}

    LAB_PATH = "${referenceLab}"

    ${waitForService}

    client.succeed("mkdir -p /root/.config/repx")

    config = f"""
    submission_target = "remote"
    [targets.local]
    base_path = "/root/repx-local"
    [targets.remote]
    address = "repxuser@target"
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "${runtime}"
    ${mountConfig}
    [targets.remote.local]
    execution_types = ["${runtime}"]
    local_concurrency = 4
    """
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    print("--- Testing ${testName} with Reference Lab ---")

    ${runCmd}

    success_count = int(target.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"Found {success_count} SUCCESS markers")

    if success_count == 0:
        raise Exception("No SUCCESS markers found! ${testName} failed.")

    ${extraValidation}

    print("\n" + "=" * 60)
    print("${bannerText}")
    print("=" * 60)
  '';
}
