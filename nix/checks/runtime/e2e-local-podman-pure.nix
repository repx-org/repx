{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-local-podman-pure";
  runtime = "podman";
  mountMode = "default";
  useSubset = true;
  bannerText = "E2E LOCAL PODMAN PURE TEST COMPLETED";
}
