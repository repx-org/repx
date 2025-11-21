{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  runs = rec {
    # Run 1: Produce numbers and calculate a sum
    simulation = repx-lib.callRun ./run-simulation.nix [ ];

    # Run 2: Analyze the results of Run 1 using repx-py
    analysis = repx-lib.callRun ./run-analysis.nix [
      [
        simulation
        "soft"
      ]
    ];
  };
}
