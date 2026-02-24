{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-impure-mode-comprehensive";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 25600;
        memorySize = 4096;
      };

      environment.systemPackages = [
        repx
        pkgs.bubblewrap
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
    default_execution_type = "bwrap"
    mount_host_paths = true
    [targets.local.local]
    execution_types = ["bwrap"]
    local_concurrency = 2
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which bwrap) /var/lib/repx-store/artifacts/host-tools/default/bin/bwrap")

    with subtest("Impure Mode on NixOS (Overlay/Union Strategy)"):
        print("--- Testing Impure Mode on NixOS with Reference Lab ---")

        machine.succeed("repx run simulation-run --lab ${referenceLab}")

        success_count = int(machine.succeed(f"find {base_path}/outputs -name SUCCESS | wc -l").strip())
        print(f"Found {success_count} SUCCESS markers")

        if success_count == 0:
            raise Exception("No SUCCESS markers found! Impure mode test failed.")

        machine.succeed(f"grep -rE '540|595' {base_path}/outputs/*/out/total_sum.txt")

    print("\n" + "=" * 60)
    print("E2E IMPURE TEST COMPLETED")
    print("=" * 60)
  '';
}
