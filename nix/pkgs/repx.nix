{ pkgs }:

let
  repx-rs = pkgs.callPackage ./repx-rs.nix { };
  repx-py = pkgs.callPackage ./repx-py.nix { };
in
pkgs.symlinkJoin {
  name = "repx";
  paths = [
    repx-rs
    repx-py
  ];
  meta = {
    mainProgram = "repx";
    description = "Reproducible HPC Experiments Framework";
  };
}
