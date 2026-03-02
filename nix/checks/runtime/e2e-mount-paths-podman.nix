{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-mount-paths-podman";
  runtime = "podman";
  mountMode = "mount-paths";
  bannerText = "E2E MOUNT PATHS PODMAN TEST COMPLETED";
}
