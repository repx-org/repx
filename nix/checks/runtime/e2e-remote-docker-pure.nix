{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-docker-pure";
  runtime = "docker";
  mountMode = "default";
  useSubset = true;
  bannerText = "E2E REMOTE DOCKER PURE TEST COMPLETED";
}
