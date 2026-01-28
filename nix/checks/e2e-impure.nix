{
  pkgs,
  repxRunner,
}:

pkgs.testers.runNixOSTest {
  name = "repx-impure-mode-comprehensive";

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
    unique_pkg_hash = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-unique-pkg"
    unique_pkg_path = f"/nix/store/{unique_pkg_hash}"
    image_rootfs = f"{base_path}/cache/images/{image_hash}/rootfs"

    machine.succeed(f"mkdir -p {image_rootfs}{unique_pkg_path}")
    machine.succeed(f"echo 'I am from image' > {image_rootfs}{unique_pkg_path}/file")
    machine.succeed(f"touch {base_path}/cache/images/{image_hash}/SUCCESS")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which bwrap) /var/lib/repx-store/artifacts/host-tools/default/bin/bwrap")


    with subtest("Impure Mode on NixOS (Overlay Strategy)"):
        print("--- Testing Impure Mode on NixOS ---")

        machine.succeed("echo 'I am host' > /etc/host-secret")

        script = f"""
        #!/bin/sh
        set -e
        if [ ! -f /etc/host-secret ]; then echo "FAIL: No host access"; exit 1; fi
        if [ ! -f {unique_pkg_path}/file ]; then echo "FAIL: No image overlay"; exit 1; fi
        if [ ! -e /nix/store ]; then echo "FAIL: No /nix/store"; exit 1; fi
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
            "--mount-host-paths"
        )

        out = machine.succeed(cmd)
        print("NixOS Test Output:", out)

        logs = machine.succeed(f"cat {base_path}/outputs/job-nixos/repx/stdout.log")
        if "PASS" not in logs:
            raise Exception("NixOS test failed")
  '';
}
