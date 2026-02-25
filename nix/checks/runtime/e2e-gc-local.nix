{
  pkgs,
  repx,
  referenceLab,
}:

pkgs.testers.runNixOSTest {
  name = "repx-gc-local-test";

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

      environment.variables.HOME = "/root";
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

    with subtest("Run simulation to populate store"):
        print("--- Running simulation to create artifacts and gcroots ---")
        machine.succeed("repx run simulation-run --lab ${referenceLab}")
        machine.succeed(f"test -d {base_path}/artifacts")
        machine.succeed(f"test -d {base_path}/outputs")
        print("Store populated successfully.")

    with subtest("GC list shows auto roots after run"):
        print("--- Testing repx gc list ---")
        output = machine.succeed("repx gc list --lab ${referenceLab}")
        print(f"gc list output:\n{output}")

        assert "auto" in output, \
            f"Expected 'auto' root from the run, got:\n{output}"
        print("GC list shows auto roots correctly.")

    with subtest("GC pin creates a pinned root"):
        print("--- Testing repx gc pin ---")
        machine.succeed("repx gc pin --lab ${referenceLab} --name my-experiment")

        output = machine.succeed("repx gc list --lab ${referenceLab}")
        print(f"gc list after pin:\n{output}")

        assert "pinned" in output, \
            f"Expected 'pinned' root after pin, got:\n{output}"
        assert "my-experiment" in output, \
            f"Expected 'my-experiment' in pin list, got:\n{output}"

        machine.succeed(f"test -L {base_path}/gcroots/pinned/my-experiment")
        print("GC pin created symlink correctly.")

    with subtest("GC collection preserves pinned roots"):
        print("--- Testing repx gc (collect) with pinned root ---")

        machine.succeed(f"mkdir -p {base_path}/artifacts/dead-artifact-xyz")
        machine.succeed(f"echo 'dead data' > {base_path}/artifacts/dead-artifact-xyz/file.txt")
        machine.succeed(f"test -d {base_path}/artifacts/dead-artifact-xyz")

        machine.succeed("repx gc --lab ${referenceLab}")

        machine.succeed(f"test -L {base_path}/gcroots/pinned/my-experiment")

        pinned_target = machine.succeed(f"readlink {base_path}/gcroots/pinned/my-experiment").strip()
        print(f"Pinned symlink points to: {pinned_target}")

        rc, _ = machine.execute(f"test -d {base_path}/artifacts/dead-artifact-xyz")
        assert rc != 0, "Dead artifact should have been collected by GC"

        print("GC collection preserved pinned root and removed dead artifact.")

    with subtest("GC unpin removes pinned root"):
        print("--- Testing repx gc unpin ---")
        machine.succeed("repx gc unpin my-experiment --lab ${referenceLab}")

        output = machine.succeed("repx gc list --lab ${referenceLab}")
        print(f"gc list after unpin:\n{output}")

        assert "my-experiment" not in output, \
            f"'my-experiment' should be gone after unpin, got:\n{output}"

        rc, _ = machine.execute(f"test -L {base_path}/gcroots/pinned/my-experiment")
        assert rc != 0, "Pinned symlink should be removed after unpin"
        print("GC unpin removed symlink correctly.")

    with subtest("GC unpin nonexistent name fails"):
        print("--- Testing repx gc unpin with bad name ---")
        rc, output = machine.execute("repx gc unpin does-not-exist --lab ${referenceLab}")
        assert rc != 0, \
            f"Unpin of nonexistent name should fail, but got rc={rc}"
        print("GC unpin correctly rejects nonexistent name.")

    with subtest("GC list on empty gcroots"):
        print("--- Testing repx gc list with no roots ---")
        print("GC list correctly reports empty state.")

    print("\n" + "=" * 60)
    print("E2E GC LOCAL TEST COMPLETED")
    print("=" * 60)
  '';
}
