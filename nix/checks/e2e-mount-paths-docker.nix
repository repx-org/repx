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
  name = "repx-mount-paths-docker";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 8192;
        memorySize = 4096;
        docker.enable = true;
      };

      environment.systemPackages = [
        repxRunner
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
    machine.succeed("ln -s $(which docker) /var/lib/repx-store/artifacts/host-tools/default/bin/docker")

    with subtest("Mount Specific Paths (Docker)"):
        print("--- Testing Mount Specific Paths (Docker) ---")

        machine.succeed("echo 'Specific Secret' > /tmp/specific-secret")

        script = """#!/bin/sh
        if [ ! -f /tmp/specific-secret ]; then echo "FAIL: No secret access"; exit 1; fi
        echo "PASS"
        """

        machine.succeed(f"mkdir -p {base_path}/job-docker-paths/bin")
        machine.succeed(f"cat <<EOF > {base_path}/job-docker-paths/bin/script.sh\n{script}\nEOF")
        machine.succeed(f"chmod +x {base_path}/job-docker-paths/bin/script.sh")
        machine.succeed(f"mkdir -p {base_path}/outputs/job-docker-paths/repx")
        machine.succeed(f"mkdir -p {base_path}/outputs/job-docker-paths/out")
        machine.succeed(f"echo '{{}}' > {base_path}/outputs/job-docker-paths/repx/inputs.json")

        cmd = (
            "repx-runner internal-execute "
            "--job-id job-docker-paths "
            f"--executable-path {base_path}/job-docker-paths/bin/script.sh "
            f"--base-path {base_path} "
            "--host-tools-dir default "
            "--runtime docker "
            f"--image-tag {image_hash} "
            "--mount-paths /tmp/specific-secret"
        )

        out = machine.succeed(cmd)
        print("Docker Paths Test Output:", out)

        logs = machine.succeed(f"cat {base_path}/outputs/job-docker-paths/repx/stdout.log")
        if "PASS" not in logs:
            raise Exception("Docker specific paths test failed")
  '';
}
