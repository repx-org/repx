{ pkgs, referenceLab }:
(pkgs.callPackage ../../../default.nix { }).overrideAttrs (_old: {
  pname = "repx-rs-tests";
  doCheck = true;
  EXAMPLE_REPX_LAB = referenceLab;
})
