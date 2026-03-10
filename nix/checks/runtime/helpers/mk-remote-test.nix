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
  bannerText ? "E2E REMOTE TEST COMPLETED",
}:

let
  isDocker = runtime == "docker";
  isPodman = runtime == "podman";
  isBwrap = runtime == "bwrap";

  mountConfig =
    if mountMode == "impure" then
      "mount_host_paths = true"
    else if mountMode == "mount-paths" then
      ''mount_paths = ["/tmp/host-data"]''
    else
      "";

  getSubsetJobs = if useSubset then pkgs.python3Packages.callPackage ./get-subset-jobs { } else null;

  targetVirtualisation = {
    diskSize = 25600;
    memorySize = 4096;
    cores = 4;
  }
  // (if isDocker then { docker.enable = true; } else { })
  // (
    if isPodman then
      {
        podman = {
          enable = true;
          dockerCompat = true;
        };
      }
    else
      { }
  );

  mountPathsSetup =
    if mountMode == "mount-paths" then
      ''
        target.succeed("mkdir -p /tmp/host-data")
        target.succeed("echo 'HOST_SECRET_DATA' > /tmp/host-data/secret.txt")
        target.succeed("chmod -R a+r /tmp/host-data")
      ''
    else
      "";

  waitForService = if isDocker then ''target.wait_for_unit("docker.service")'' else "";

  subsetImport = pkgs.lib.optionalString useSubset "from get_subset_jobs import get_subset_jobs";

  runCmd =
    if useSubset then
      ''
        subset_jobs = get_subset_jobs("${referenceLab}")
        if not subset_jobs:
            raise Exception("get_subset_jobs returned empty list for ${testName}")
        run_args = " ".join(subset_jobs)
        print(f"Running subset: {run_args}")
        client.succeed(f"repx run {run_args} --lab ${referenceLab}")
      ''
    else
      ''client.succeed("repx run ${runName} --lab ${referenceLab}")'';

  dhcpDenyInterfaces = [
    "veth*"
  ]
  ++ (if isDocker then [ "docker*" ] else [ ])
  ++ (if isPodman then [ "podman*" ] else [ ]);

  extraGroups = (if isDocker then [ "docker" ] else [ ]) ++ (if isPodman then [ "podman" ] else [ ]);

  hostToolSymlinks =
    if isBwrap then
      ''
        target.succeed(f"ln -s $(which bwrap) {base_path}/artifacts/host-tools/default/bin/bwrap")
      ''
    else if isDocker then
      ''
        target.succeed(f"ln -s $(which docker) {base_path}/artifacts/host-tools/default/bin/docker")
      ''
    else
      ''
        target.succeed(f"ln -s $(which podman) {base_path}/artifacts/host-tools/default/bin/podman")
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
        virtualisation = targetVirtualisation;

        networking.dhcpcd.denyInterfaces = dhcpDenyInterfaces;

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
          inherit extraGroups;
          password = "password";
          home = "/home/repxuser";
          createHome = true;
        };
      };
  };

  testScript = ''
    ${subsetImport}
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
    ${hostToolSymlinks}
    target.succeed(f"chown -R repxuser:users {base_path}")

    ${mountPathsSetup}

    LAB_PATH = "${referenceLab}"

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
    client.succeed("mkdir -p /root/.config/repx")
    client.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    print("--- Testing ${testName} with Reference Lab ---")

    ${waitForService}

    ${runCmd}

    success_count = int(target.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"Found {success_count} SUCCESS markers")

    if success_count == 0:
        print("Output tree:")
        print(target.succeed(f"find {base_path}/outputs -maxdepth 4"))
        print("Stderr logs:")
        print(target.succeed(f"find {base_path}/outputs -name 'stderr.log' -exec echo '--- {{}} ---' \\; -exec cat {{}} \\;"))
        raise Exception("No SUCCESS markers found! ${testName} failed.")

    ${extraValidation}

    print("\n" + "=" * 60)
    print("${bannerText}")
    print("=" * 60)
  '';
}
