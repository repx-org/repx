{ pkgs, referenceLab }:
pkgs.repx-py.override {
  reference-lab = referenceLab;
}
