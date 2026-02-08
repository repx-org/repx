{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-impure-mode-podman";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 25600;
        memorySize = 4096;
        podman = {
          enable = true;
          dockerCompat = true;
        };
      };

      environment.systemPackages = [
        repx
        pkgs.podman
        pkgs.jq
      ];
    };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    machine.succeed("mkdir -p /root/.config/repx")

    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "podman"
    mount_host_paths = true
    [targets.local.local]
    execution_types = ["podman"]
    local_concurrency = 2
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which podman) /var/lib/repx-store/artifacts/host-tools/default/bin/podman")

    with subtest("Impure Mode (Podman)"):
        print("--- Testing Impure Mode (Podman) with Reference Lab ---")

        machine.succeed("repx run simulation-run --lab ${referenceLab}")

        success_count = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
        print(f"Found {success_count} SUCCESS markers")

        if success_count == 0:
            raise Exception("No SUCCESS markers found! Podman impure mode test failed.")

    print("\n" + "=" * 60)
    print("E2E IMPURE PODMAN TEST COMPLETED")
    print("=" * 60)
  '';
}
