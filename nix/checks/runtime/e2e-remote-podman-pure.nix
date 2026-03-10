{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-podman-pure";
  runtime = "podman";
  mountMode = "default";
  useSubset = true;
  bannerText = "E2E REMOTE PODMAN PURE TEST COMPLETED";
}
