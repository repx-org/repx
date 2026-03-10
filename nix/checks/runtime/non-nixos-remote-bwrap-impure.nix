{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-non-nixos-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "non-nixos-remote-bwrap-impure";
  runtime = "bwrap";
  mountMode = "impure";
  useSubset = true;
  bannerText = "NON-NIXOS REMOTE BWRAP IMPURE TEST COMPLETED";
}
