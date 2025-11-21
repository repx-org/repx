{ pkgs, repx-lib }:
let
  producerStageFile = pkgs.writeText "stage-producer.nix" ''
    { pkgs, ... }:
    {
      pname = "test-producer";
      version = "0.1";
      outputs = { out1 = "$out/data.txt"; };

      run = { outputs, ... }: '''
        echo "Hello World" > "''${outputs.out1}"
      ''';
    }
  '';

  consumerStageFile = pkgs.writeText "stage-consumer.nix" ''
    { pkgs, ... }:
    {
      pname = "test-consumer";
      version = "0.1";
      inputs = {
        input1 = "";
      };
      run = { inputs, ... }: '''
        cat "''${inputs.input1}" > "$out/result.txt"
      ''';
    }
  '';

  pipelineDef = pkgs.writeText "pipeline.nix" ''
    { repx }:
    let
      stage1 = repx.callStage ${producerStageFile} [];
      stage2 = repx.callStage ${consumerStageFile} [
        [ stage1 "out1" "input1" ]
      ];
    in
    repx.mkPipe {
      inherit stage1 stage2;
    }
  '';

  runDefFile = pkgs.writeText "run-definition.nix" ''
    { pkgs, ... }:
    {
      containerized = false;

      params = {
        iteration = [ 1 ];
      };

      pipelines = [ "${pipelineDef}" ];
    }
  '';

  labResult = repx-lib.mkLab {
    inherit pkgs repx-lib;
    gitHash = "0000000";
    runs = {
      run1 = repx-lib.callRun runDefFile [ ];
    };
  };

in
labResult.labNative
