{ pkgs, repx-lib }:
let
  helpers = repx-lib.mkPipelineHelpers {
    inherit pkgs repx-lib;
  };

  typoStage = pkgs.writeText "stage-typo-resources.nix" ''
    { pkgs }:
    {
      pname = "typo-resources";
      resources = {
        mem = "4G";
        cpuss = 2;
      };
      outputs = { out = "$out/done"; };
      run = { outputs, ... }: "touch ''${outputs.out}";
    }
  '';

  multiTypoStage = pkgs.writeText "stage-multi-typo-resources.nix" ''
    { pkgs }:
    {
      pname = "multi-typo-resources";
      resources = {
        memory = "4G";
        cores = 2;
        timeout = "01:00:00";
      };
      outputs = { out = "$out/done"; };
      run = { outputs, ... }: "touch ''${outputs.out}";
    }
  '';

  tryCall = stageFile: builtins.tryEval (helpers.callStage stageFile [ ]);

  resultTypo = tryCall typoStage;
  resultMultiTypo = tryCall multiTypoStage;
in
pkgs.runCommand "check-resource-hints" { } ''
  echo "Testing Resource Hint Validation..."

  check_failure() {
    name="$1"
    success="$2"
    if [ "$success" == "true" ]; then
      echo "FAIL [$name]: Expected error for invalid resource keys, but succeeded."
      exit 1
    else
      echo "PASS [$name]: Correctly threw error."
    fi
  }

  check_failure "Single typo (cpuss)" "${toString resultTypo.success}"
  check_failure "Multiple typos (memory, cores, timeout)" "${toString resultMultiTypo.success}"

  echo "All resource hint validation checks passed."
  touch $out
''
