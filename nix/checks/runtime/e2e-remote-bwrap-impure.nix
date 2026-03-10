{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-remote-bwrap-impure";
  runtime = "bwrap";
  mountMode = "impure";
  useSubset = true;
  bannerText = "E2E REMOTE BWRAP IMPURE TEST COMPLETED";
}
