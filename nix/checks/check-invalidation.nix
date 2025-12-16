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
    (import ../reference-lab/lab.nix {
      inherit pkgs;
      repx-lib = patchedRepxLib;
      gitHash = "test";
    }).lab;

  baselineLab = buildLabWithPatches { };

  getHashes =
    jobsList: pname:
    let
      matches = pkgs.lib.filter (d: d.pname == pname) jobsList;
    in
    pkgs.lib.sort (a: b: a < b) (map (d: builtins.baseNameOf (toString d)) matches);

  baselineHashes = pname: getHashes baselineLab.passthru.allJobDerivations pname;

  allChanged = old: new: (pkgs.lib.intersectLists old new) == [ ];

  noneChanged = old: new: old == new;

  partialChanged =
    old: new:
    let
      common = pkgs.lib.intersectLists old new;
    in
    (common != [ ]) && (common != old);

  scenarios = {
    mod_analysis =
      let
        lab = buildLabWithPatches {
          stagePatches = {
            "stage-analysis.nix" = {
              version = "patched";
            };
          };
        };
        hashes = pname: getHashes lab.passthru.allJobDerivations pname;
      in
      {
        name = "Modify Analysis Stage";
        assertions = [
          (allChanged (baselineHashes "stage-analysis") (hashes "stage-analysis"))
          (noneChanged (baselineHashes "stage-E-total-sum") (hashes "stage-E-total-sum"))
        ];
      };

    mod_upstream =
      let
        lab = buildLabWithPatches {
          stagePatches = {
            "stage-D-scatter-sum.nix" = {
              version = "patched";
            };
          };
        };
        hashes = pname: getHashes lab.passthru.allJobDerivations pname;
      in
      {
        name = "Modify Upstream Stage D";
        assertions = [
          (allChanged (baselineHashes "stage-D-partial-sums") (hashes "stage-D-partial-sums"))
          (allChanged (baselineHashes "stage-E-total-sum") (hashes "stage-E-total-sum"))
          (allChanged (baselineHashes "stage-analysis") (hashes "stage-analysis"))
        ];
      };

    mod_utils_granular =
      let
        headerA_orig = pkgs.writeTextDir "header_a.txt" "Alpha\n";
        headerA_mod = pkgs.writeTextDir "header_a.txt" "Alpha MODIFIED\n";
        headerB_orig = pkgs.writeTextDir "header_b.txt" "Beta\n";

        mockDirs =
          contentMap:
          let
            mkWrapper =
              name: src:
              let
                wrapper = pkgs.runCommandLocal "wrapper-${name}" { } ''
                  mkdir -p $out/${name}
                  cp -rT ${src} $out/${name}
                '';
              in
              {
                path = "${wrapper}/${name}";
                drv = wrapper;
              };
            objects = pkgs.lib.mapAttrsToList mkWrapper contentMap;
          in
          {
            _repx_param = true;
            values = map (x: x.path) objects;
            context = map (x: x.drv) objects;
          };

        mkLab =
          a: b:
          buildLabWithPatches {
            utilsOverrides = {
              dirs =
                base: _args: src:
                if (pkgs.lib.hasSuffix "pkgs/headers" (toString src)) then
                  mockDirs {
                    "header_a" = a;
                    "header_b" = b;
                  }
                else
                  base.dirs src;
            };
          };

        labBase = mkLab headerA_orig headerB_orig;
        labMod = mkLab headerA_mod headerB_orig;
        hashes = lab: pname: getHashes lab.passthru.allJobDerivations pname;
      in
      {
        name = "Modify Utils Dirs (Granular Invalidation)";
        assertions = [
          (partialChanged (hashes labBase "stage-A-producer") (hashes labMod "stage-A-producer"))

          (partialChanged (hashes labBase "stage-C-consumer") (hashes labMod "stage-C-consumer"))

          (noneChanged (hashes labBase "stage-B-producer") (hashes labMod "stage-B-producer"))
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
