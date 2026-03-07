{ pkgs, repx-lib }:
let
  utils = repx-lib.mkUtils { inherit pkgs; };

  mkTestRun =
    params:
    repx-lib.mkRun {
      inherit pkgs;
      repx-lib = repx-lib // {
        inherit utils;
      };
      name = "zip-test";
      containerized = false;
      pipelines = [
        ({ repx }: repx.mkPipe { })
      ];
      inherit params;
    };

  happyRun = mkTestRun {
    workload = [
      "a"
      "b"
      "c"
    ];
    config = utils.zip {
      vf_enable = [
        0
        1
      ];
      label = [
        "baseline"
        "vf"
      ];
    };
  };

  happyCount = builtins.length happyRun.runs;

  multiZipRun = mkTestRun {
    group_a = utils.zip {
      x = [
        1
        2
      ];
      y = [
        "a"
        "b"
      ];
    };
    group_b = utils.zip {
      p = [
        10
        20
        30
      ];
      q = [
        "X"
        "Y"
        "Z"
      ];
    };
  };

  multiZipCount = builtins.length multiZipRun.runs;

  zipOnlyRun = mkTestRun {
    config = utils.zip {
      a = [
        1
        2
        3
      ];
      b = [
        "x"
        "y"
        "z"
      ];
    };
  };
  zipOnlyCount = builtins.length zipOnlyRun.runs;

  noZipRun = mkTestRun {
    x = [
      1
      2
    ];
    y = [
      "a"
      "b"
      "c"
    ];
  };
  noZipCount = builtins.length noZipRun.runs;

  collisionZipVsNormal = builtins.tryEval (mkTestRun {
    mode = [
      "x"
      "y"
    ];
    config = utils.zip {
      mode = [
        "a"
        "b"
      ];
      label = [
        "L1"
        "L2"
      ];
    };
  });

  collisionZipVsZip = builtins.tryEval (mkTestRun {
    g1 = utils.zip {
      mode = [
        "a"
        "b"
      ];
      x = [
        1
        2
      ];
    };
    g2 = utils.zip {
      mode = [
        "c"
        "d"
      ];
      y = [
        3
        4
      ];
    };
  });

  collisionAnchorVsMember = builtins.tryEval (mkTestRun {
    mode = utils.zip {
      mode = [
        "a"
        "b"
      ];
      label = [
        "L1"
        "L2"
      ];
    };
  });

  mismatchedLengths = builtins.tryEval (
    utils.zip {
      a = [
        1
        2
        3
      ];
      b = [
        "x"
        "y"
      ];
    }
  );

in
pkgs.runCommand "check-zip-params" { } ''
  echo "Testing utils.zip parameter behavior..."
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

  check_fail() {
    name="$1"; success="$2"
    if [ "$success" == "true" ]; then
      echo "FAIL [$name]: expected error, but succeeded"
      fail=1
    else
      echo "PASS [$name]: correctly rejected"
    fi
  }

  check_eq "zip+cartesian count" "${toString happyCount}" "6"
  check_eq "multi-zip count" "${toString multiZipCount}" "6"
  check_eq "zip-only count" "${toString zipOnlyCount}" "3"
  check_eq "no-zip backwards compat" "${toString noZipCount}" "6"

  check_fail "zip member vs normal param" "${toString collisionZipVsNormal.success}"
  check_fail "zip member vs zip member" "${toString collisionZipVsZip.success}"
  check_fail "anchor key vs member" "${toString collisionAnchorVsMember.success}"
  check_fail "mismatched zip lengths" "${toString mismatchedLengths.success}"

  if [ "$fail" -ne 0 ]; then
    echo "SOME TESTS FAILED"
    exit 1
  fi

  echo "All utils.zip tests passed."
  touch $out
''
