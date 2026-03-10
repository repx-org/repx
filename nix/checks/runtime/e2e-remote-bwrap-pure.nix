{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-bwrap-pure";
  runtime = "bwrap";
  mountMode = "default";
  useSubset = true;
  bannerText = "E2E REMOTE BWRAP PURE TEST COMPLETED";
}
