{ pkgs, repx-lib }:
let
  producer = pkgs.runCommand "mock-producer" {
    passthru = {
      pname = "mock-producer";
      outputMetadata = {
        "out_src" = "$out/src";
      };
    };
  } "touch $out";

  consumerDefFile = pkgs.writeText "stage-consumer.nix" ''
    { pkgs }:
    {
      pname = "mock-consumer";
      inputs = {
        "in_tgt" = "";
      };
      outputs = {
        "out" = "$out/res";
      };
      run = { ... }: "touch $out/res";
    }
  '';

  helpers = repx-lib.mkPipelineHelpers {
    inherit pkgs repx-lib;
  };

  attemptBadConnection = helpers.callStage consumerDefFile [ producer ];

  result = builtins.tryEval attemptBadConnection;

  consumerDefFile2 = pkgs.writeText "stage-consumer-2.nix" ''
    { pkgs }:
    {
      pname = "mock-consumer-2";
      inputs = {
        "common" = "";
        "missing" = "";
      };
      outputs = { "out" = "$out/res"; };
      run = { ... }: "touch $out/res";
    }
  '';

  producer2 = pkgs.runCommand "mock-producer-2" {
    passthru = {
      pname = "mock-producer-2";
      outputMetadata = {
        "common" = "$out/common";
      };
    };
  } "touch $out";

  attemptUnresolved = helpers.callStage consumerDefFile2 [ producer2 ];
  result2 = builtins.tryEval attemptUnresolved;

in
pkgs.runCommand "check-pipeline-logic" { } ''
  echo "Testing Implicit Dependency Error Logic..."

  if [ "${toString result.success}" == "true" ]; then
    echo "FAIL [Case 1]: Expected error when connecting mismatched stages implicitly, but succeeded."
    exit 1
  else
    echo "PASS [Case 1]: Implicit dependency mismatch correctly threw an error."
  fi

  if [ "${toString result2.success}" == "true" ]; then
    echo "FAIL [Case 2]: Expected error for unresolved inputs, but succeeded."
    exit 1
  else
    echo "PASS [Case 2]: Unresolved inputs correctly threw an error."
  fi

  touch $out
''
