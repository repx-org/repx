{
  pkgs,
  repx-lib,
  gitHash,
}:

let
  simulationRun = (pkgs.callPackage ./nix/runs/run-simulation.nix { inherit repx-lib; });
in
repx-lib.mkLab {
  inherit pkgs gitHash;
  runs = [
    simulationRun
  ];
}
