{
  pkgs,
  repx-lib,
  gitHash,
}:

let
  runs = rec {
    simulation = repx-lib.callRun ./runs/run-simulation.nix [ ];
    analysis = repx-lib.callRun ./runs/run-analysis.nix [
      [
        simulation
        "soft"
      ]
    ];
  };
in
repx-lib.mkLab {
  inherit
    pkgs
    gitHash
    repx-lib
    runs
    ;
  lab_version = "1.0.0";
  groups = {
    all = with runs; [
      simulation
      analysis
    ];
    compute = with runs; [ simulation ];
  };
}
