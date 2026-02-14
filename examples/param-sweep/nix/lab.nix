{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  lab_version = "1.0.0";
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
