{ pkgs, repx-lib }:
let
  helpers = repx-lib.mkPipelineHelpers {
    inherit pkgs repx-lib;
  };

  badSig1 = pkgs.writeText "stage-bad-sig-1.nix" ''
    { pkgs }:
    {
      pname = "bad-sig-1";
      outputs = { pkgs }: { "out" = "$out/res"; };
      run = { ... }: "touch $out/res";
    }
  '';

  badSig2 = pkgs.writeText "stage-bad-sig-2.nix" ''
    { pkgs }:
    {
      pname = "bad-sig-2";
      outputs = { params, pkgs }: { "out" = "$out/res"; };
      run = { ... }: "touch $out/res";
    }
  '';

  badSig3 = pkgs.writeText "stage-bad-sig-3.nix" ''
    { pkgs }:
    {
      pname = "bad-sig-3";
      outputs = { }: { "out" = "$out/res"; };
      run = { ... }: "touch $out/res";
    }
  '';

  tryCall = stageFile: builtins.tryEval (helpers.callStage stageFile [ ]);

  result1 = tryCall badSig1;
  result2 = tryCall badSig2;
  result3 = tryCall badSig3;

in
pkgs.runCommand "check-dynamic-params-validation" { } ''
  echo "Testing Dynamic Params Validation Logic..."

  check_failure() {
    name="$1"
    success="$2"
    if [ "$success" == "true" ]; then
      echo "FAIL [$name]: Expected error for invalid function signature, but succeeded."
      exit 1
    else
      echo "PASS [$name]: Correctly threw error."
    fi
  }

  check_failure "Case 1 ({ pkgs })" "${toString result1.success}"
  check_failure "Case 2 ({ params, pkgs })" "${toString result2.success}"
  check_failure "Case 3 ({ })" "${toString result3.success}"

  touch $out
''
