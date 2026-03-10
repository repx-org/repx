{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-local-docker-impure";
  runtime = "docker";
  mountMode = "impure";
  useSubset = true;
  bannerText = "E2E LOCAL DOCKER IMPURE TEST COMPLETED";
}
