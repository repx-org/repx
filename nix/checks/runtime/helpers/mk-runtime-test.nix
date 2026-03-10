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
  isBwrap = runtime == "bwrap";

  mountConfig =
    if mountMode == "impure" then
      "mount_host_paths = true"
    else if mountMode == "mount-paths" then
      ''mount_paths = ["/tmp/host-data"]''
    else
      "";

  runtimeBinary =
    if isBwrap then
      "bwrap"
    else if isDocker then
      "docker"
    else
      "podman";

  extraPackages =
    (if isBwrap then [ pkgs.bubblewrap ] else [ ])
    ++ (if isPodman then [ pkgs.podman ] else [ ])
    ++ [ pkgs.jq ];

  getSubsetJobs = if useSubset then pkgs.python3Packages.callPackage ./get-subset-jobs { } else null;

  virtualisation = {
    diskSize = 25600;
    memorySize = 4096;
    cores = 2;
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
        machine.succeed("mkdir -p /tmp/host-data")
        machine.succeed("echo 'HOST_SECRET_DATA' > /tmp/host-data/secret.txt")
      ''
    else
      "";

  waitForService = if isDocker then ''machine.wait_for_unit("docker.service")'' else "";

  subsetImport = pkgs.lib.optionalString useSubset "from get_subset_jobs import get_subset_jobs";

  runCmd =
    if useSubset then
      ''
        subset_jobs = get_subset_jobs("${referenceLab}")
        if not subset_jobs:
            raise Exception("get_subset_jobs returned empty list for ${testName}")
        run_args = " ".join(subset_jobs)
        print(f"Running subset: {run_args}")
        machine.succeed(f"repx run {run_args} --lab ${referenceLab}")
      ''
    else
      ''machine.succeed("repx run ${runName} --lab ${referenceLab}")'';
in

pkgs.testers.runNixOSTest {
  name = testName;

  extraPythonPackages = _: pkgs.lib.optional useSubset getSubsetJobs;

  nodes.machine = _: {
    inherit virtualisation;
    environment.systemPackages = [ repx ] ++ extraPackages;
  };

  testScript = ''
    start_all()

    ${subsetImport}

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    ${mountPathsSetup}

    machine.succeed("mkdir -p /root/.config/repx")

    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "${runtime}"
    ${mountConfig}
    [targets.local.local]
    execution_types = ["${runtime}"]
    local_concurrency = 4
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which ${runtimeBinary}) /var/lib/repx-store/artifacts/host-tools/default/bin/${runtimeBinary}")

    print("--- Testing ${testName} with Reference Lab ---")

    ${waitForService}

    ${runCmd}

    success_count = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
    print(f"Found {success_count} SUCCESS markers")

    if success_count == 0:
        raise Exception("No SUCCESS markers found! ${testName} failed.")

    ${extraValidation}

    print("\n" + "=" * 60)
    print("${bannerText}")
    print("=" * 60)
  '';
}
