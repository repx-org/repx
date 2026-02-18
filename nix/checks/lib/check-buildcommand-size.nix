{ pkgs, ... }:

let
  repx-lib = import ../../lib/main.nix;

  simpleStage = _: {
    pname = "stage";
    params = {
      run_id = 0;
      stage_id = 0;
    };

    outputs = {
      "result" = "$out/result.txt";
    };

    run =
      { outputs, params, ... }:
      ''
        echo "run=${toString params.run_id} stage=${toString params.stage_id}" > "${outputs.result}"
      '';
  };

  pipeline =
    { repx }:
    repx.mkPipe {
      stage = repx.callStage simpleStage [ ];
    };

  mkRun =
    runIndex:
    { repx-lib, ... }:
    let
      inherit (repx-lib) utils;
    in
    {
      name = "run-${toString runIndex}";
      pipelines = [ pipeline ];
      params = {
        run_id = [ runIndex ];
        stage_id = utils.range 1 8;
      };
    };

  runCount = 60;
  runIndices = pkgs.lib.range 1 runCount;

  runs = builtins.listToAttrs (
    map (i: {
      name = "run-${toString i}";
      value = repx-lib.callRun (mkRun i) [ ];
    }) runIndices
  );

  lab = repx-lib.mkLab {
    inherit pkgs repx-lib;
    gitHash = "buildcommand-size-test";
    lab_version = "1.0.0";
    inherit runs;
  };

in
lab.lab
