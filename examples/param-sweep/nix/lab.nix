{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  runs = rec {
    sweep_run = repx-lib.callRun ./run-sweep.nix [ ];

    plot_run = repx-lib.callRun ./run-plot.nix [
      [
        sweep_run
        "soft"
      ]
    ];
  };
}
