{
  pkgs,
  repx-lib,
  revision,
}:

let
  simulationRun = (pkgs.callPackage ./nix/runs/run-simulation.nix { inherit repx-lib; });
in
repx-lib.mkLab {
  inherit pkgs revision;
  runs = [
    simulationRun
  ];
}
