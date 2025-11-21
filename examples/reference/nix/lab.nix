{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  runs = rec {
    simulation = repx-lib.callRun ./runs/run-simulation.nix [ ];
    analysis = repx-lib.callRun ./runs/run-analysis.nix [
      [
        simulation
        "soft"
      ]
    ];
  };
}
