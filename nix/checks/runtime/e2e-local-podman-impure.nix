{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-local-podman-impure";
  runtime = "podman";
  mountMode = "impure";
  useSubset = true;
  bannerText = "E2E LOCAL PODMAN IMPURE TEST COMPLETED";
}
