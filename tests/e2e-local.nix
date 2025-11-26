{
  pkgs,
  repxRunner,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-e2e-container-test";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation.diskSize = 4096;
      virtualisation.memorySize = 2048;
      virtualisation.docker.enable = true;
      virtualisation.podman.enable = true;

      environment.systemPackages = [
        repxRunner
        pkgs.jq
      ];

      environment.variables.HOME = "/root";
    };

  testScript = ''
    start_all()


    def configure_repx(runtime):
        config = f"""
        submission_target = "local"
        [targets.local]
        base_path = "/var/lib/repx-store"
        default_scheduler = "local"
        default_execution_type = "{runtime}"
        [targets.local.local]
        execution_types = ["{runtime}"]
        local_concurrency = 2
        """
        machine.succeed("mkdir -p /root/.config/repx")
        machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    with subtest("Docker Execution"):
        print("--- Starting Docker E2E Test ---")
        configure_repx("docker")

        machine.wait_for_unit("docker.service")

        machine.succeed("repx-runner run simulation-run --lab ${referenceLab}")

        machine.succeed("grep -rE '400|415' /var/lib/repx-store/outputs/*/out/total_sum.txt")

        machine.succeed("rm -rf /var/lib/repx-store/outputs/*")
        machine.succeed("rm -rf /var/lib/repx-store/cache/*")

    with subtest("Podman Execution"):
        print("--- Starting Podman E2E Test ---")
        configure_repx("podman")

        machine.succeed("repx-runner run simulation-run --lab ${referenceLab}")

        machine.succeed("grep -rE '400|415' /var/lib/repx-store/outputs/*/out/total_sum.txt")
  '';
}
