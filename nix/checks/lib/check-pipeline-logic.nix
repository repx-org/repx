{ pkgs, repx-lib }:
let
  producer = {
    _repx_virtual_job = true;
    jobId = "mock-producer-hash";
    jobName = "mock-producer-1.1";
    jobDirName = "mock-producer-hash-mock-producer-1.1";
    pname = "mock-producer";
    repxStageType = "simple";
    resolvedParameters = { };
    outputMetadata = {
      "out_src" = "$out/src";
    };
    executables = {
      main = {
        inputs = [ ];
        outputs = {
          "out_src" = "$out/src";
        };
      };
    };
    scriptDrv = pkgs.writeTextDir "bin/mock-producer" "#!/bin/bash\ntrue";
    resources = null;
    parametersJson = "{}";
    dependencyManifestJson = "[]";
  };

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

  producer2 = {
    _repx_virtual_job = true;
    jobId = "mock-producer2-hash";
    jobName = "mock-producer-2-1.1";
    jobDirName = "mock-producer2-hash-mock-producer-2-1.1";
    pname = "mock-producer-2";
    repxStageType = "simple";
    resolvedParameters = { };
    outputMetadata = {
      "common" = "$out/common";
    };
    executables = {
      main = {
        inputs = [ ];
        outputs = {
          "common" = "$out/common";
        };
      };
    };
    scriptDrv = pkgs.writeTextDir "bin/mock-producer-2" "#!/bin/bash\ntrue";
    resources = null;
    parametersJson = "{}";
    dependencyManifestJson = "[]";
  };

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
