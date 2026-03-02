{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-mount-paths-specific";
  runtime = "bwrap";
  mountMode = "mount-paths";
  bannerText = "E2E MOUNT PATHS TEST COMPLETED";
}
