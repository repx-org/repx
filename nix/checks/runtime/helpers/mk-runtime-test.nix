{
  pkgs,
  repx,
  referenceLab,
  testName,
  runtime,
  mountMode,
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
    else
      ''mount_paths = ["/tmp/specific-secret"]'';

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

  virtualisation = {
    diskSize = 25600;
    memorySize = 4096;
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
      ''machine.succeed("echo 'Specific Secret' > /tmp/specific-secret")''
    else
      "";

  waitForService = if isDocker then ''machine.wait_for_unit("docker.service")'' else "";
in

pkgs.testers.runNixOSTest {
  name = testName;

  nodes.machine = _: {
    inherit virtualisation;
    environment.systemPackages = [ repx ] ++ extraPackages;
  };

  testScript = ''
    start_all()

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
    local_concurrency = 2
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which ${runtimeBinary}) /var/lib/repx-store/artifacts/host-tools/default/bin/${runtimeBinary}")

    with subtest("${testName}"):
        print("--- Testing ${testName} with Reference Lab ---")

        ${waitForService}

        machine.succeed("repx run simulation-run --lab ${referenceLab}")

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
