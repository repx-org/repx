{ pkgs, ... }:

let
  repx-lib = import ../../lib/main.nix;

  simpleStage = _: {
    pname = "simple-stage";
    params = {
      param_a = 0;
      param_b = 0;
      param_c = 0;
    };

    outputs = {
      "result" = "$out/result.txt";
    };

    run =
      { outputs, params, ... }:
      ''
        echo "${toString params.param_a}-${toString params.param_b}" > "${outputs.result}"
      '';
  };

  pipeline =
    { repx }:
    repx.mkPipe {
      stage = repx.callStage simpleStage [ ];
    };

  run =
    { repx-lib, ... }:
    let
      inherit (repx-lib) utils;
    in
    {
      name = "large-lab-run";
      pipelines = [ pipeline ];
      params = {
        param_a = utils.range 1 20;
        param_b = utils.range 1 20;
      };
    };

  lab = repx-lib.mkLab {
    inherit pkgs repx-lib;
    gitHash = "large-lab-test";
    lab_version = "1.0.0";
    runs = {
      large = repx-lib.callRun run [ ];
    };
  };

in
lab.lab
