{ pkgs, repx-lib }:
let
  buildLabWithPatches =
    {
      stagePatches ? { },
      utilsOverrides ? { },
    }:
    let
      patchedRepxLib = repx-lib // {
        mkUtils =
          args:
          let
            base = repx-lib.mkUtils args;
          in
          base // (pkgs.lib.mapAttrs (_name: fn: fn base args) utilsOverrides);

        mkPipelineHelpers =
          args:
          let
            helpers = repx-lib.mkPipelineHelpers args;
          in
          helpers
          // {
            callStage =
              stageFile: deps:
              let
                name = baseNameOf (toString stageFile);
                isTarget = builtins.hasAttr name stagePatches;
                newStageDef =
                  if isTarget then (args: (import stageFile args) // stagePatches.${name}) else stageFile;
              in
              helpers.callStage newStageDef deps;
          };
      };
    in
    builtins.tryEval
      (import ../../reference-lab/lab.nix {
        inherit pkgs;
        repx-lib = patchedRepxLib;
        gitHash = "test";
      }).lab;

  baselineEval = buildLabWithPatches { };

  modAnalysis = buildLabWithPatches {
    stagePatches = {
      "stage-analysis.nix" = {
        version = "patched";
      };
    };
  };

  modUpstream = buildLabWithPatches {
    stagePatches = {
      "stage-D-scatter-sum.nix" = {
        version = "patched";
      };
    };
  };

  modResources = buildLabWithPatches {
    stagePatches = {
      "stage-B-producer.nix" = {
        resources = {
          mem = "64G";
          cpus = 32;
          time = "48:00:00";
          partition = "gpu";
        };
      };
    };
  };

in
pkgs.runCommand "check-invalidation" { } ''
  echo "Running Invalidation Tests..."
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

  check_eq "baseline eval" "${toString baselineEval.success}" "1"
  check_eq "mod analysis stage" "${toString modAnalysis.success}" "1"
  check_eq "mod upstream stage D" "${toString modUpstream.success}" "1"
  check_eq "mod resources (no invalidation)" "${toString modResources.success}" "1"

  if [ "$fail" -ne 0 ]; then
    echo "SOME TESTS FAILED"
    exit 1
  fi

  echo ""
  echo "All invalidation eval tests passed."
  echo "NOTE: Hash propagation correctness is tested in Rust (crates/repx-expand)."
  touch $out
''
