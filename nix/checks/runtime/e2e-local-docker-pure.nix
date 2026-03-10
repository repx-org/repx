{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-local-docker-pure";
  runtime = "docker";
  mountMode = "default";
  useSubset = true;
  bannerText = "E2E LOCAL DOCKER PURE TEST COMPLETED";
}
