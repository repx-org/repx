{
  pkgs,
  repx-lib,
  gitHash,
}:

let
  wrapNative =
    runPath: args:
    let
      runDef = import runPath args;
    in
    runDef // { containerized = false; };
in
repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  lab_version = "1.0.0";
  runs = rec {
    simulation = repx-lib.callRun (wrapNative ./runs/run-simulation.nix) [ ];
    analysis = repx-lib.callRun (wrapNative ./runs/run-analysis.nix) [
      [
        simulation
        "soft"
      ]
    ];
  };
}
