{
  pkgs,
  repxRunner,
}:

pkgs.testers.runNixOSTest {
  name = "repx-mount-paths-specific";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 4096;
        memorySize = 2048;
      };

      environment.systemPackages = [
        repxRunner
        pkgs.bubblewrap
        pkgs.jq
      ];
    };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    image_hash = "fake-image"
    image_rootfs = f"{base_path}/cache/images/{image_hash}/rootfs"
    machine.succeed(f"mkdir -p {image_rootfs}")
    machine.succeed(f"touch {base_path}/cache/images/{image_hash}/SUCCESS")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which bwrap) /var/lib/repx-store/artifacts/host-tools/default/bin/bwrap")

    with subtest("Mount Specific Paths on NixOS"):
        print("--- Testing Mount Specific Paths on NixOS ---")

        # Create a specific secret file
        machine.succeed("echo 'Specific Secret' > /tmp/specific-secret")

        script = """
        #!/bin/sh
        set -e
        if [ ! -f /tmp/specific-secret ]; then echo "FAIL: No secret access"; exit 1; fi
        echo "PASS"
        """

        machine.succeed(f"mkdir -p {base_path}/job-nixos/bin")
        machine.succeed(f"cat <<EOF > {base_path}/job-nixos/bin/script.sh\n{script}\nEOF")
        machine.succeed(f"chmod +x {base_path}/job-nixos/bin/script.sh")
        machine.succeed(f"mkdir -p {base_path}/outputs/job-nixos/repx")
        machine.succeed(f"mkdir -p {base_path}/outputs/job-nixos/out")
        machine.succeed(f"echo '{{}}' > {base_path}/outputs/job-nixos/repx/inputs.json")

        cmd = (
            "repx-runner internal-execute "
            "--job-id job-nixos "
            f"--executable-path {base_path}/job-nixos/bin/script.sh "
            f"--base-path {base_path} "
            "--host-tools-dir default "
            "--runtime bwrap "
            f"--image-tag {image_hash} "
            "--mount-paths /tmp/specific-secret "
            "--mount-paths /nix/store "
            "--mount-paths /bin"
        )

        out = machine.succeed(cmd)
        print("NixOS Test Output:", out)

        logs = machine.succeed(f"cat {base_path}/outputs/job-nixos/repx/stdout.log")
        if "PASS" not in logs:
            raise Exception("NixOS test failed")
  '';
}
