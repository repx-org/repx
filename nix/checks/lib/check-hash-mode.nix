{ pkgs }:
let
  repx-lib = import ../../lib/main.nix;
  utils = repx-lib.mkUtils { inherit pkgs; };

  mkTestRun =
    {
      hashMode ? "pure",
      runDeps ? [ ],
      parameters ? { },
    }:
    builtins.tryEval (
      repx-lib.mkRun {
        inherit pkgs hashMode;
        repx-lib = repx-lib // {
          inherit utils;
        };
        name = "hash-mode-test";
        interRunDepTypes = { };
        dependencyJobs = { };
        pipelines = [
          (_: {
            stages = {
              compute = {
                pname = "compute";
                version = "1.0";
                resolvedParameters = parameters;
                runDependencies = runDeps;
                outputs = {
                  result = "$out/result.txt";
                };
                run =
                  { outputs, ... }:
                  ''
                    echo "done" > "${outputs.result}"
                  '';
              };
            };
          })
        ];
        inherit parameters;
      }
    );

  pureEval = mkTestRun { hashMode = "pure"; };
  paramsEval = mkTestRun { hashMode = "params-only"; };
  pureWithDeps = mkTestRun {
    hashMode = "pure";
    runDeps = [ pkgs.hello ];
  };
  paramsWithDeps = mkTestRun {
    hashMode = "params-only";
    runDeps = [ pkgs.hello ];
  };
  paramsWithParams = mkTestRun {
    hashMode = "params-only";
    parameters = {
      x = utils.list [ "value" ];
    };
  };

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

  if [ "$fail" -ne 0 ]; then
    echo "SOME TESTS FAILED"
    exit 1
  fi

  echo ""
  echo "All hash mode tests passed."
  echo "NOTE: Hash stability/propagation/invalidation tests are in Rust (crates/repx-expand)."
  touch $out
''
