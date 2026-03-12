{ pkgs, referenceLab }:
pkgs.callPackage ../../pkgs/repx-py.nix {
  reference-lab = referenceLab;
}
