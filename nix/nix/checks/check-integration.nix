{ pkgs, ... }:
(import ../reference-lab/lab.nix {
  inherit pkgs;
  repx-lib = import ../../lib/main.nix;
  gitHash = "integration-test";
}).lab
