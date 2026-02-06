{
  pkgs,
  repx,
}:

let
  staticBusybox = pkgs.pkgsStatic.busybox;

  staticBwrap = pkgs.pkgsStatic.bubblewrap;

  busyboxImage = pkgs.dockerTools.buildImage {
    name = "busybox";
    tag = "latest";
    copyToRoot = [ pkgs.busybox ];
    config = {
      Cmd = [ "${pkgs.busybox}/bin/sh" ];
    };
  };

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
        docker.enable = true;
        podman.enable = true;
      };

      networking.dhcpcd.denyInterfaces = [
        "veth*"
        "docker*"
        "podman*"
      ];

      environment.systemPackages = [
        pkgs.bubblewrap
        pkgs.jq
      ];
    };

  testScript = ''
    start_all()

    base_path = "/var/lib/repx-store"
    machine.succeed(f"mkdir -p {base_path}")

    machine.succeed(f"mkdir -p {base_path}/bin")
    machine.succeed(f"cp ${repxBinary} {base_path}/bin/repx")
    machine.succeed(f"chmod +x {base_path}/bin/repx")

    image_hash = "test-image"
    image_rootfs = f"{base_path}/cache/images/{image_hash}/rootfs"
    machine.succeed(f"mkdir -p {image_rootfs}/bin")
    machine.succeed(f"mkdir -p {image_rootfs}/usr/bin")
    machine.succeed(f"mkdir -p {image_rootfs}/tmp")
    machine.succeed(f"cp ${staticBusybox}/bin/busybox {image_rootfs}/bin/")
    machine.succeed(f"ln -s /bin/busybox {image_rootfs}/bin/sh")
    machine.succeed(f"ln -s /bin/busybox {image_rootfs}/bin/cat")
    machine.succeed(f"ln -s /bin/busybox {image_rootfs}/bin/echo")
    machine.succeed(f"ln -s /bin/busybox {image_rootfs}/bin/test")
    machine.succeed(f"ln -s /bin/busybox {image_rootfs}/bin/ls")
    machine.succeed(f"ln -s /bin/busybox {image_rootfs}/bin/mkdir")
    machine.succeed(f"touch {base_path}/cache/images/{image_hash}/SUCCESS")

    machine.succeed(f"mkdir -p {base_path}/artifacts/host-tools/default/bin")
    machine.succeed(f"cp ${staticBwrap}/bin/bwrap {base_path}/artifacts/host-tools/default/bin/bwrap")
    machine.succeed(f"chmod +x {base_path}/artifacts/host-tools/default/bin/bwrap")
    machine.succeed(f"ln -s $(which docker) {base_path}/artifacts/host-tools/default/bin/docker")
    machine.succeed(f"ln -s $(which podman) {base_path}/artifacts/host-tools/default/bin/podman")

    docker_image_hash = "busybox_latest"
    machine.succeed(f"mkdir -p {base_path}/artifacts/images")
    machine.copy_from_host("${busyboxImage}", f"{base_path}/artifacts/images/{docker_image_hash}.tar")

    machine.succeed("mkdir -p /tmp/host-data")
    machine.succeed("echo 'HOST_SECRET_DATA' > /tmp/host-data/secret.txt")

    machine.succeed("mkdir -p /var/lib/fake-bin")
    machine.succeed("cp ${staticBusybox}/bin/busybox /var/lib/fake-bin/busybox")
    machine.succeed("ln -s busybox /var/lib/fake-bin/sh")
    machine.succeed("ln -s busybox /var/lib/fake-bin/cat")
    machine.succeed("ln -s busybox /var/lib/fake-bin/mkdir")
    machine.succeed("mkdir -p /opt/specific-mount")
    machine.succeed("echo 'SPECIFIC_MOUNT_DATA' > /opt/specific-mount/data.txt")

    def create_job(job_id, script_content):
        """Helper to create job structure for internal-execute"""
        machine.succeed(f"mkdir -p {base_path}/jobs/{job_id}/bin")
        machine.succeed(f"cat <<'SCRIPT_EOF' > {base_path}/jobs/{job_id}/bin/script.sh\n{script_content}\nSCRIPT_EOF")
        machine.succeed(f"chmod +x {base_path}/jobs/{job_id}/bin/script.sh")
        machine.succeed(f"mkdir -p {base_path}/outputs/{job_id}/repx")
        machine.succeed(f"mkdir -p {base_path}/outputs/{job_id}/out")
        machine.succeed(f"echo '{{}}' > {base_path}/outputs/{job_id}/repx/inputs.json")

    def run_repx_without_nix(cmd):
        """Run repx inside bwrap WITHOUT /nix access - simulating non-NixOS"""
        bwrap_cmd = (
            "bwrap "
            "--bind / / "
            "--dev-bind /dev /dev "
            "--tmpfs /nix "
            "--bind /var/lib/fake-bin /bin "
            f"-- {base_path}/bin/repx {cmd}"
        )
        return bwrap_cmd

    def run_test(job_id, runtime, image_tag, mount_mode):
        """Run a test with repx having NO access to /nix"""
        cmd_parts = [
            "internal-execute",
            f"--job-id {job_id}",
            f"--executable-path {base_path}/jobs/{job_id}/bin/script.sh",
            f"--base-path {base_path}",
            "--host-tools-dir default",
            f"--runtime {runtime}",
        ]

        if image_tag:
            cmd_parts.append(f"--image-tag {image_tag}")

        if mount_mode == "impure":
            cmd_parts.append("--mount-host-paths")
        elif mount_mode == "specific":
            cmd_parts.append("--mount-paths /opt/specific-mount")
            cmd_parts.append("--mount-paths /tmp/host-data")

        repx_args = " ".join(cmd_parts)
        full_cmd = run_repx_without_nix(repx_args)

        print(f"\n>>> Running test: {job_id} (runtime={runtime}, mount={mount_mode})")
        print(f"Command: {full_cmd}")

        machine.succeed(full_cmd)

        logs = machine.succeed(f"cat {base_path}/outputs/{job_id}/repx/stdout.log")
        print(f"Output: {logs}")
        if "PASS" not in logs:
            stderr = machine.succeed(f"cat {base_path}/outputs/{job_id}/repx/stderr.log 2>/dev/null || true")
            print(f"STDERR: {stderr}")
            raise Exception(f"Test {job_id} did not output PASS")
        print(f"✓ Test {job_id} PASSED")

    simple_script = """#!/bin/sh
    echo "Running inside container..."
    echo "PASS"
    """

    impure_script = """#!/bin/sh
    set -e
    if [ ! -f /tmp/host-data/secret.txt ]; then
        echo "FAIL: Cannot access host file"
        exit 1
    fi
    cat /tmp/host-data/secret.txt
    echo "PASS"
    """

    specific_script = """#!/bin/sh
    set -e
    if [ ! -f /opt/specific-mount/data.txt ]; then
        echo "FAIL: Cannot access specific mount"
        exit 1
    fi
    cat /opt/specific-mount/data.txt
    echo "PASS"
    """

    with subtest("Bwrap - Pure Mode"):
        create_job("bwrap-pure", simple_script)
        run_test("bwrap-pure", "bwrap", image_hash, "pure")

    with subtest("Bwrap - Impure Mode"):
        create_job("bwrap-impure", impure_script)
        run_test("bwrap-impure", "bwrap", image_hash, "impure")

    with subtest("Bwrap - Specific Mounts"):
        create_job("bwrap-specific", specific_script)
        run_test("bwrap-specific", "bwrap", image_hash, "specific")

    print("\n" + "=" * 60)
    print("NON-NIXOS STANDALONE TEST PASSED!")
    print("repx (static binary) successfully ran on simulated non-NixOS environment")
    print("(Tested: bwrap pure, impure, specific modes)")
    print("=" * 60)
  '';
}
