{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-impure-mode-docker";
  runtime = "docker";
  mountMode = "impure";
  bannerText = "E2E IMPURE DOCKER TEST COMPLETED";
}
