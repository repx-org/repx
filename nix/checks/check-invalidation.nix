{ pkgs, repx-lib }:
let
  buildLabWithPatches =
    patches:
    let
      patchedRepxLib = repx-lib // {
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
                isTarget = builtins.hasAttr name patches;
                newStageDef = if isTarget then (args: (import stageFile args) // patches.${name}) else stageFile;
              in
              helpers.callStage newStageDef deps;
          };
      };
    in
    (import ../reference-lab/lab.nix {
      inherit pkgs;
      repx-lib = patchedRepxLib;
      gitHash = "test";
    }).lab;

  baselineLab = buildLabWithPatches { };
  baselineJobs = pkgs.lib.listToAttrs (
    map (drv: pkgs.lib.nameValuePair drv.pname drv) baselineLab.passthru.allJobDerivations
  );

  getHash = jobs: pname: builtins.baseNameOf (toString jobs.${pname});

  scenarios = {
    mod_analysis =
      let
        lab = buildLabWithPatches {
          "stage-analysis.nix" = {
            version = "patched";
          };
        };
        jobs = pkgs.lib.listToAttrs (
          map (drv: pkgs.lib.nameValuePair drv.pname drv) lab.passthru.allJobDerivations
        );
      in
      {
        name = "Modify Analysis Stage";
        assertions = [
          (getHash jobs "stage-analysis" != getHash baselineJobs "stage-analysis")
          (getHash jobs "stage-E-total-sum" == getHash baselineJobs "stage-E-total-sum")
        ];
      };

    mod_upstream =
      let
        lab = buildLabWithPatches {
          "stage-D-scatter-sum.nix" = {
            version = "patched";
          };
        };
        jobs = pkgs.lib.listToAttrs (
          map (drv: pkgs.lib.nameValuePair drv.pname drv) lab.passthru.allJobDerivations
        );
      in
      {
        name = "Modify Upstream Stage D";
        assertions = [
          (getHash jobs "stage-D-partial-sums" != getHash baselineJobs "stage-D-partial-sums")
          (getHash jobs "stage-E-total-sum" != getHash baselineJobs "stage-E-total-sum")
          (getHash jobs "stage-analysis" != getHash baselineJobs "stage-analysis")
        ];
      };
  };

  runScenarios = pkgs.lib.mapAttrsToList (
    _key: sc:
    if pkgs.lib.all (x: x) sc.assertions then
      "PASS: ${sc.name}"
    else
      throw "FAIL: Scenario '${sc.name}' failed assertions."
  ) scenarios;

in
pkgs.runCommand "check-invalidation" { } ''
  echo "Running Invalidation Tests..."
  ${pkgs.lib.concatMapStringsSep "\n" (msg: "echo '${msg}'") runScenarios}
  touch $out
''
