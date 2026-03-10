{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-podman-impure";
  runtime = "podman";
  mountMode = "impure";
  useSubset = true;
  bannerText = "E2E REMOTE PODMAN IMPURE TEST COMPLETED";
}
