{
  pkgs,
  repxRunner,
}:

let
  testImage = pkgs.dockerTools.buildImage {
    name = "busybox";
    tag = "latest";
    copyToRoot = [ pkgs.busybox ];
    config = {
      Cmd = [ "${pkgs.busybox}/bin/sh" ];
    };
  };
in
pkgs.testers.runNixOSTest {
  name = "repx-impure-mode-podman";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 8192;
        memorySize = 4096;
        podman = {
          enable = true;
          dockerCompat = true;
        };
      };

      environment.systemPackages = [
        repxRunner
        pkgs.podman
        pkgs.jq
      ];
    };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    image_hash = "busybox_latest"
    machine.succeed(f"mkdir -p {base_path}/artifacts/images")

    machine.copy_from_host("${testImage}", f"{base_path}/artifacts/images/{image_hash}.tar")

    machine.succeed("mkdir -p /var/lib/repx-store/artifacts/host-tools/default/bin")
    machine.succeed("ln -s $(which podman) /var/lib/repx-store/artifacts/host-tools/default/bin/podman")

    with subtest("Impure Mode (Podman)"):
        print("--- Testing Impure Mode (Podman) ---")
        machine.succeed("echo 'I am host' > /tmp/host-secret")

        script = """#!/bin/sh
        if [ ! -f /tmp/host-secret ]; then echo "FAIL: No host access"; exit 1; fi
        echo "PASS"
        """

        machine.succeed(f"mkdir -p {base_path}/job-podman/bin")
        machine.succeed(f"cat <<EOF > {base_path}/job-podman/bin/script.sh\n{script}\nEOF")
        machine.succeed(f"chmod +x {base_path}/job-podman/bin/script.sh")
        machine.succeed(f"mkdir -p {base_path}/outputs/job-podman/repx")
        machine.succeed(f"mkdir -p {base_path}/outputs/job-podman/out")
        machine.succeed(f"echo '{{}}' > {base_path}/outputs/job-podman/repx/inputs.json")

        cmd = (
            "repx-runner internal-execute "
            "--job-id job-podman "
            f"--executable-path {base_path}/job-podman/bin/script.sh "
            f"--base-path {base_path} "
            "--host-tools-dir default "
            "--runtime podman "
            f"--image-tag {image_hash} "
            "--mount-host-paths"
        )

        out = machine.succeed(cmd)
        print("Podman Test Output:", out)

        logs = machine.succeed(f"cat {base_path}/outputs/job-podman/repx/stdout.log")
        if "PASS" not in logs:
            raise Exception("Podman test failed")
  '';
}
