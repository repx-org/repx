{ pkgs, repx-lib }:
let
  utils = repx-lib.mkUtils { inherit pkgs; };

  mkTestRun =
    parameters:
    repx-lib.mkRun {
      inherit pkgs;
      repx-lib = repx-lib // {
        inherit utils;
      };
      name = "non-scalar-test";
      containerized = false;
      pipelines = [
        ({ repx }: repx.mkPipe { })
      ];
      inherit parameters;
    };

  nestedList = builtins.tryEval (mkTestRun {
    workload = [
      [
        1
        2
      ]
      [
        3
        4
      ]
    ];
  });

  attrsetInList = builtins.tryEval (mkTestRun {
    workload = [
      { a = "1"; }
      { b = "2"; }
    ];
  });

  mixedParams = builtins.tryEval (mkTestRun {
    mode = [
      "fast"
      "slow"
    ];
    config = [
      { threads = "4"; }
      { threads = "8"; }
    ];
  });

  allScalar = builtins.tryEval (mkTestRun {
    mode = [
      "fast"
      "slow"
    ];
    count = [
      1
      2
      3
    ];
  });

  allScalarCount = if allScalar.success then builtins.length allScalar.value.runs else -1;

in
pkgs.runCommand "check-non-scalar-parameters" { } ''
  echo "Testing non-scalar parameter rejection in mkRun..."
  fail=0

  check_fail() {
    name="$1"; success="$2"
    if [ "$success" == "true" ]; then
      echo "FAIL [$name]: expected error, but succeeded"
      fail=1
    else
      echo "PASS [$name]: correctly rejected"
    fi
  }

  check_eq() {
    name="$1"; got="$2"; expected="$3"
    if [ "$got" != "$expected" ]; then
      echo "FAIL [$name]: expected $expected, got $got"
      fail=1
    else
      echo "PASS [$name]"
    fi
  }

  check_fail "nested list in parameter" "${toString nestedList.success}"
  check_fail "attrset in parameter list" "${toString attrsetInList.success}"
  check_fail "mixed scalar and non-scalar" "${toString mixedParams.success}"
  check_eq  "all-scalar params succeed" "${toString allScalar.success}" "1"
  check_eq  "all-scalar combination count" "${toString allScalarCount}" "6"

  if [ "$fail" -ne 0 ]; then
    echo "SOME TESTS FAILED"
    exit 1
  fi

  echo "All non-scalar parameter tests passed."
  touch $out
''
