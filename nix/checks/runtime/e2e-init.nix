{
  pkgs,
  repx,
  repx-lib,
  gitHash,
}:

let
  initLab =
    (import ../../../crates/repx-cli/templates/init/nix/lab.nix {
      inherit pkgs repx-lib gitHash;
    }).lab;
in
pkgs.testers.runNixOSTest {
  name = "repx-e2e-init-test";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 25600;
        memorySize = 2048;
      };

      environment.systemPackages = [
        repx
        pkgs.bubblewrap
        pkgs.git
        pkgs.jq
      ];

      environment.variables.HOME = "/root";
    };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"

    machine.succeed("mkdir -p /root/.config/repx")
    config = f"""
    submission_target = "local"
    [targets.local]
    base_path = "{base_path}"
    default_scheduler = "local"
    default_execution_type = "native"
    [targets.local.local]
    execution_types = ["native"]
    local_concurrency = 2
    """
    machine.succeed(f"cat <<EOF > /root/.config/repx/config.toml\n{config}\nEOF")

    with subtest("repx init creates project structure"):
        print("--- Testing repx init scaffolding ---")

        machine.succeed("repx init /tmp/test-project --name my-experiment")

        machine.succeed("test -f /tmp/test-project/flake.nix")
        machine.succeed("test -f /tmp/test-project/.envrc")
        machine.succeed("test -f /tmp/test-project/.gitignore")
        machine.succeed("test -f /tmp/test-project/nix/lab.nix")
        machine.succeed("test -f /tmp/test-project/nix/run.nix")
        machine.succeed("test -f /tmp/test-project/nix/pipeline.nix")
        machine.succeed("test -f /tmp/test-project/nix/stage-hello.nix")

        machine.succeed("test -d /tmp/test-project/.git")

        machine.succeed("grep 'my-experiment' /tmp/test-project/flake.nix")

        print("All init files created correctly.")

    with subtest("repx init refuses to overwrite"):
        print("--- Testing repx init refuses existing project ---")

        rc, output = machine.execute("repx init /tmp/test-project 2>&1")
        assert rc != 0, "repx init should fail when flake.nix already exists"
        assert "already exists" in output, f"Expected 'already exists' error, got: {output}"

        print("Init correctly refuses to overwrite.")

    with subtest("repx init defaults name to directory"):
        print("--- Testing repx init name defaults ---")

        machine.succeed("repx init /tmp/another-project")
        machine.succeed("grep 'another-project' /tmp/another-project/flake.nix")

        print("Name defaulting works correctly.")

    with subtest("Run the init template lab"):
        print("--- Testing repx run with init template lab ---")

        machine.succeed("repx run main-run --lab ${initLab}")

        machine.succeed(f"grep -r 'Hello from repx!' {base_path}/outputs/*/out/greeting.txt")

        print("Init template lab ran successfully.")

    print("\n" + "=" * 60)
    print("E2E INIT TEST COMPLETED")
    print("=" * 60)
  '';
}
