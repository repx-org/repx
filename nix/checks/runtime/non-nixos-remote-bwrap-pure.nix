{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-non-nixos-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "non-nixos-remote-bwrap-pure";
  runtime = "bwrap";
  mountMode = "pure";
  useSubset = true;
  bannerText = "NON-NIXOS REMOTE BWRAP PURE TEST COMPLETED";
}
