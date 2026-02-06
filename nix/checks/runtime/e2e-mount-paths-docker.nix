{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-mount-paths-docker";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 8192;
        memorySize = 4096;
        docker.enable = true;
      };

      environment.systemPackages = [
        repx
        pkgs.jq
      ];
    };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    machine.succeed("echo 'Specific Secret' > /tmp/specific-secret")

    machine.succeed("mkdir -p /root/.config/repx")

    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "docker"
    mount_paths = ["/tmp/specific-secret"]
    [targets.local.local]
    execution_types = ["docker"]
    local_concurrency = 2
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which docker) /var/lib/repx-store/artifacts/host-tools/default/bin/docker")

    with subtest("Mount Specific Paths (Docker)"):
        print("--- Testing Mount Specific Paths (Docker) with Reference Lab ---")

        machine.wait_for_unit("docker.service")

        machine.succeed("repx run simulation-run --lab ${referenceLab}")

        success_count = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
        print(f"Found {success_count} SUCCESS markers")

        if success_count == 0:
            raise Exception("No SUCCESS markers found! Docker mount paths test failed.")

    print("\n" + "=" * 60)
    print("E2E MOUNT PATHS DOCKER TEST COMPLETED")
    print("=" * 60)
  '';
}
