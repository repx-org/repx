{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-mount-paths-docker";
  runtime = "docker";
  mountMode = "mount-paths";
  bannerText = "E2E MOUNT PATHS DOCKER TEST COMPLETED";
}
