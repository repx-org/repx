{
  pkgs,
  repx,
  referenceLab,
}:

let
  staticBusybox = pkgs.pkgsStatic.busybox;
  staticBwrap = pkgs.pkgsStatic.bubblewrap;
  repxBinary = "${repx}/bin/repx";
in
pkgs.testers.runNixOSTest {
  name = "non-nixos-standalone";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 10240;
        memorySize = 4096;
        cores = 2;
      };

      environment.systemPackages = [
        pkgs.bubblewrap
        pkgs.jq
      ];

      users.users.repxuser = {
        isNormalUser = true;
        home = "/home/repxuser";
        createHome = true;
      };
    };

  testScript = ''
    start_all()

    base_path = "/home/repxuser/repx-store"
    local_lab = "/home/repxuser/lab"

    machine.succeed("mkdir -p /host-root")
    for d in ["bin", "etc", "usr", "var", "tmp", "home", "run", "dev", "proc"]:
        machine.succeed(f"mkdir -p /host-root/{d}")

    machine.succeed("chown root:root /host-root")
    machine.succeed("chmod 755 /host-root")

    machine.succeed(f"mkdir -p {base_path}/bin")
    machine.succeed(f"mkdir -p {local_lab}")

    machine.succeed(f"cp -rL ${referenceLab}/* {local_lab}/")

    machine.succeed(f"cp ${repxBinary} {base_path}/bin/repx")
    machine.succeed(f"chmod +x {base_path}/bin/repx")

    machine.succeed(f"mkdir -p {base_path}/artifacts/host-tools/default/bin")
    machine.succeed(f"cp ${staticBwrap}/bin/bwrap {base_path}/artifacts/host-tools/default/bin/bwrap")
    machine.succeed(f"chmod +x {base_path}/artifacts/host-tools/default/bin/bwrap")

    machine.succeed("cp ${staticBusybox}/bin/busybox /host-root/bin/busybox")
    for cmd in ["sh", "cat", "mkdir", "echo", "find", "grep", "ls", "rm", "cp", "chmod", "pwd", "env", "test", "true", "false", "sleep"]:
        machine.succeed(f"ln -sf busybox /host-root/bin/{cmd}")

    machine.succeed(f"chown -R repxuser:users {base_path}")
    machine.succeed(f"chown -R repxuser:users {local_lab}")

    def run_repx_without_nix(cmd):
        """Run repx inside bwrap WITHOUT /nix - simulating non-NixOS as a normal user with NO write access to root"""
        bwrap_cmd = (
            "sudo -u repxuser bwrap "
            "--unshare-all "
            "--share-net "
            "--bind /host-root / "
            "--bind /etc /etc "
            "--bind /usr /usr "
            "--bind /var /var "
            "--bind /tmp /tmp "
            "--bind /home /home "
            "--bind /run /run "
            "--dev-bind /dev /dev "
            "--proc /proc "
            f"-- {base_path}/bin/repx {cmd}"
        )
        return bwrap_cmd

    with subtest("Bwrap - Impure Mode on Non-NixOS (no host /nix)"):
        print("--- Testing Impure Mode on simulated non-NixOS ---")
        print("Expected: Success (Bug should be fixed with --tmpfs /nix)")

        config = f"""
            submission_target = "local"
            [targets.local]
            base_path = "{base_path}"
            default_scheduler = "local"
            default_execution_type = "bwrap"
            mount_host_paths = true
            [targets.local.local]
            execution_types = ["bwrap"]
            local_concurrency = 1
        """
        machine.succeed("mkdir -p /home/repxuser/.config/repx")
        machine.succeed(f"cat <<'EOF' > /home/repxuser/.config/repx/config.toml\n{config}\nEOF")
        machine.succeed("chown -R repxuser:users /home/repxuser/.config")

        cmd = run_repx_without_nix(f"run simulation-run --lab {local_lab}")
        print(f"Command: {cmd}")

        rc, output = machine.execute(f"RUST_LOG=repx_executor=debug {cmd}")

        print(f"Return code: {rc}")
        print(f"Output:\n{output}")

        logs = machine.succeed(f"find {base_path}/outputs -name 'stderr.log' -exec cat {{}} \\; 2>/dev/null || true")
        if logs:
            print(f"Stderr logs:\n{logs}")

        if rc == 0:
            print("Test passed - checking for SUCCESS marker")
            success_files = machine.succeed(f"find {base_path}/outputs -name SUCCESS").strip()
            if success_files:
                print(f"✓ SUCCESS: Bug is fixed! Job output found:\n{success_files}")
            else:
                raise Exception("repx returned 0 but no job SUCCESS marker found")
        else:
            if "Permission denied" in output or "Can't mkdir /nix" in output or \
               "Permission denied" in logs or "Can't mkdir /nix" in logs:
                print("FAILURE: Bug is still present: bwrap can't mkdir /nix")
            else:
                print("Failed but with DIFFERENT error. Investigating...")
                if "signal 6" in output or "ABRT" in output:
                     print("repx crashed (SIGABRT/Panic).")
            raise Exception("Test failed")

    print("\n" + "=" * 60)
    print("NON-NIXOS STANDALONE TEST COMPLETED")
    print("=" * 60)
  '';
}
