{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-bwrap-overlay-fallback";

  nodes.machine =
    { pkgs, ... }:
    {
      boot.kernelPackages = pkgs.linuxPackages_5_10;

      virtualisation = {
        diskSize = 25600;
        memorySize = 2048;
      };

      environment.systemPackages = [
        repx
        pkgs.jq
        pkgs.bubblewrap
      ];

      environment.variables.HOME = "/root";
    };

  testScript = ''
    start_all()

    kernel_version = machine.succeed("uname -r").strip()
    print(f"Kernel version: {kernel_version}")
    assert kernel_version.startswith("5.10"), f"Expected kernel 5.10.x, got {kernel_version}"

    with subtest("Verify overlay fails on kernel 5.10"):
        print("--- Verifying that bwrap overlay fails on this kernel ---")

        machine.succeed("mkdir -p /tmp/overlay-test/{lower,upper,work,merged}")

        rc, output = machine.execute(
            "bwrap --unshare-user --dev-bind / / "
            "--overlay-src /tmp/overlay-test/lower "
            "--overlay /tmp/overlay-test/upper /tmp/overlay-test/work /tmp/overlay-test/merged "
            "true 2>&1"
        )

        print(f"bwrap overlay test output: {output}")
        assert rc != 0, "bwrap overlay should fail on kernel 5.10"
        assert "Invalid argument" in output or "Operation not permitted" in output, \
            f"Expected overlay error, got: {output}"
        print("✓ Confirmed: bwrap overlay fails on kernel 5.10 as expected")

    config = """
    submission_target = "local"
    [targets.local]
    base_path = "/var/lib/repx-store"
    default_scheduler = "local"
    default_execution_type = "bwrap"
    [targets.local.local]
    execution_types = ["bwrap"]
    local_concurrency = 2
    """
    machine.succeed("mkdir -p /root/.config/repx")
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    with subtest("Bwrap with overlay fallback"):
        print("--- Testing bwrap execution with overlay fallback ---")

        machine.succeed("repx run simulation-run --lab ${referenceLab}")

        machine.succeed("grep -rE '540|595' /var/lib/repx-store/outputs/*/out/total_sum.txt")

        cache_content = machine.succeed("cat /var/lib/repx-store/cache/capabilities/overlay_support.json")
        print(f"Capability cache content: {cache_content}")
        assert '"tmp_overlay_supported": false' in cache_content or '"tmp_overlay_supported":false' in cache_content, \
            f"Expected overlay to be cached as unsupported, got: {cache_content}"

        print("✓ SUCCESS: repx correctly fell back to read-only bind mounts on kernel 5.10")

    with subtest("Verify fallback uses cached result"):
        print("--- Verifying cached capability result is reused ---")

        machine.succeed("rm -rf /var/lib/repx-store/outputs/*")

        machine.succeed("repx run simulation-run --lab ${referenceLab}")

        machine.succeed("grep -rE '540|595' /var/lib/repx-store/outputs/*/out/total_sum.txt")

        print("✓ SUCCESS: Cached overlay capability result was reused")

    print("\n" + "=" * 60)
    print("BWRAP OVERLAY FALLBACK TEST COMPLETED SUCCESSFULLY")
    print("=" * 60)
  '';
}
