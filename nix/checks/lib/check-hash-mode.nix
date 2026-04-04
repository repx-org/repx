{ pkgs }:
let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };
  mkScatterGatherStage = import ../../lib/stage-scatter-gather.nix { inherit pkgs; };

  mkPipeline =
    {
      runDepsA ? [ ],
      parameters ? { },
    }:
    let
      stageA = mkSimpleStage {
        pname = "stage-A";
        version = "1.0";
        resolvedParameters = parameters;
        runDependencies = runDepsA;
        outputs = {
          result = "$out/result.txt";
        };
        run =
          { outputs, ... }:
          ''
            echo "A" > "${outputs.result}"
          '';
      };
    in
    {
      inherit stageA;
    };

  mkSG =
    {
      runDeps ? [ ],
      parameters ? { },
    }:
    builtins.tryEval (mkScatterGatherStage {
      pname = "sg-test";
      version = "1.0";
      resolvedParameters = parameters;

      scatter = {
        pname = "sg-scatter";
        outputs = {
          worker__arg = {
            startIndex = 0;
          };
          work__items = "$out/work_items.json";
        };
        run =
          { outputs, ... }:
          ''
            echo '[{"startIndex": 0}]' > "${outputs.work__items}"
          '';
        runDependencies = runDeps;
      };

      steps = {
        compute = {
          pname = "sg-compute";
          inputs = {
            worker__item = "";
          };
          outputs = {
            partial = "$out/partial.txt";
          };
          run =
            { outputs, ... }:
            ''
              echo "computed" > "${outputs.partial}"
            '';
          runDependencies = runDeps;
        };
      };

      gather = {
        pname = "sg-gather";
        inputs = {
          worker__outs = "[]";
        };
        outputs = {
          final = "$out/final.txt";
        };
        run =
          { outputs, ... }:
          ''
            echo "gathered" > "${outputs.final}"
          '';
        runDependencies = runDeps;
      };
    });

  pureEval = builtins.tryEval (mkPipeline {
    hashMode = "pure";
  });
  paramsEval = builtins.tryEval (mkPipeline {
    hashMode = "params-only";
  });
  pureWithDeps = builtins.tryEval (mkPipeline {
    hashMode = "pure";
    runDepsA = [ pkgs.hello ];
  });
  paramsWithDeps = builtins.tryEval (mkPipeline {
    hashMode = "params-only";
    runDepsA = [ pkgs.hello ];
  });
  paramsWithParams = builtins.tryEval (mkPipeline {
    hashMode = "params-only";
    parameters = {
      x = "value";
    };
  });

  sgPure = mkSG { hashMode = "pure"; };
  sgParams = mkSG { hashMode = "params-only"; };
  sgPureWithDeps = mkSG {
    hashMode = "pure";
    runDeps = [ pkgs.hello ];
  };
  sgParamsWithDeps = mkSG {
    hashMode = "params-only";
    runDeps = [ pkgs.hello ];
  };

  pureScriptDrv = pureEval.value.stageA.scriptDrv;
  paramsScriptDrv = paramsEval.value.stageA.scriptDrv;

in
pkgs.runCommand "check-hash-mode" { } ''
  echo "Running Hash Mode Tests..."
  fail=0

  check_eq() {
    name="$1"; got="$2"; expected="$3"
    if [ "$got" != "$expected" ]; then
      echo "FAIL [$name]: expected $expected, got $got"
      fail=1
    else
      echo "PASS [$name]"
    fi
  }

  check_eq "pure mode eval" "${toString pureEval.success}" "1"
  check_eq "params-only mode eval" "${toString paramsEval.success}" "1"
  check_eq "pure mode with deps" "${toString pureWithDeps.success}" "1"
  check_eq "params-only with deps" "${toString paramsWithDeps.success}" "1"
  check_eq "params-only with params" "${toString paramsWithParams.success}" "1"

  test -e "${pureScriptDrv}" && echo "PASS: pure scriptDrv exists" || { echo "FAIL: pure scriptDrv missing"; fail=1; }
  test -e "${paramsScriptDrv}" && echo "PASS: params scriptDrv exists" || { echo "FAIL: params scriptDrv missing"; fail=1; }

  check_eq "SG pure eval" "${toString sgPure.success}" "1"
  check_eq "SG params-only eval" "${toString sgParams.success}" "1"
  check_eq "SG pure with deps" "${toString sgPureWithDeps.success}" "1"
  check_eq "SG params-only with deps" "${toString sgParamsWithDeps.success}" "1"

  if [ "$fail" -ne 0 ]; then
    echo "SOME TESTS FAILED"
    exit 1
  fi

  echo ""
  echo "All hash mode eval tests passed."
  echo "NOTE: Hash stability/propagation/invalidation tests are in Rust (crates/repx-expand)."
  touch $out
''
