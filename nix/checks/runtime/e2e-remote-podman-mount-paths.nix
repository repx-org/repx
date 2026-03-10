{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-podman-mount-paths";
  runtime = "podman";
  mountMode = "mount-paths";
  runName = "mount-paths-run";
  extraValidation = ''
    mount_check = target.succeed(f"cat $(find {base_path}/outputs -name mount_check_result.txt | head -1)").strip()
    if mount_check != "HOST_SECRET_DATA":
        raise Exception(f"Mount path check failed! Expected 'HOST_SECRET_DATA', got '{mount_check}'")
    print(f"Mount path validation passed: job read '{mount_check}' from mounted path")
  '';
  bannerText = "E2E REMOTE PODMAN MOUNT-PATHS TEST COMPLETED";
}
