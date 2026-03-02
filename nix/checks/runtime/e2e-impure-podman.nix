{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-impure-mode-podman";
  runtime = "podman";
  mountMode = "impure";
  bannerText = "E2E IMPURE PODMAN TEST COMPLETED";
}
