{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-docker-impure";
  runtime = "docker";
  mountMode = "impure";
  useSubset = true;
  bannerText = "E2E REMOTE DOCKER IMPURE TEST COMPLETED";
}
