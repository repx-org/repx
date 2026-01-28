{ pkgs, repxRunner }:

pkgs.testers.runNixOSTest {
  name = "check-foreign-distro-compat";

  nodes.machine =
    { pkgs, ... }:
    {
      environment.systemPackages = [ pkgs.bubblewrap ];
    };

  testScript = ''
    start_all()

    print("Simulating non-NixOS environment (no /nix/store except the binary itself)...")

    binary_path = "${repxRunner}/bin/repx-runner"

    real_binary = machine.succeed(f"readlink -f {binary_path}").strip()
    print(f"Resolved binary path: {real_binary}")

    cmd = (
        "bwrap "
        "--unshare-user --unshare-ipc --unshare-pid --unshare-uts --unshare-net "
        f"--ro-bind {real_binary} /repx-runner "
        "--dev /dev "
        "--tmpfs /tmp "
        "/repx-runner --version"
    )

    output = machine.succeed(cmd)
    print("PASS: Binary ran successfully in isolation")
    print(output)
  '';
}
