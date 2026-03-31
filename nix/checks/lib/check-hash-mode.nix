{ pkgs }:
let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };
  mkScatterGatherStage = import ../../lib/stage-scatter-gather.nix { inherit pkgs; };

  mkPipeline =
    {
      hashMode ? "pure",
      pnameA ? "stage-A",
      pnameB ? "stage-B",
      pnameC ? "stage-C",
      versionA ? "1.0",
      versionB ? "1.0",
      versionC ? "1.0",
      runDepsA ? [ ],
      runDepsB ? [ ],
      runDepsC ? [ ],
      parameters ? { },
    }:
    let
      stageA = mkSimpleStage {
        pname = pnameA;
        version = versionA;
        resolvedParameters = parameters;
        inherit hashMode;
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

      stageB = mkSimpleStage {
        pname = pnameB;
        version = versionB;
        resolvedParameters = parameters;
        inherit hashMode;
        runDependencies = runDepsB;
        upstreamJobs = [ stageA ];
        dependencyDerivations = [ stageA.scriptDrv ];
        inputMappings = [
          {
            type = "intra-pipeline";
            job_id = stageA.jobDirName;
            source_output = "result";
            target_input = "a_result";
          }
        ];
        inputs = {
          a_result = "";
        };
        outputs = {
          result = "$out/result.txt";
        };
        run =
          { inputs, outputs, ... }:
          ''
            cat "${inputs.a_result}" > "${outputs.result}"
            echo "B" >> "${outputs.result}"
          '';
      };

      stageC = mkSimpleStage {
        pname = pnameC;
        version = versionC;
        resolvedParameters = parameters;
        inherit hashMode;
        runDependencies = runDepsC;
        upstreamJobs = [ stageB ];
        dependencyDerivations = [ stageB.scriptDrv ];
        inputMappings = [
          {
            type = "intra-pipeline";
            job_id = stageB.jobDirName;
            source_output = "result";
            target_input = "b_result";
          }
        ];
        inputs = {
          b_result = "";
        };
        outputs = {
          result = "$out/result.txt";
        };
        run =
          { inputs, outputs, ... }:
          ''
            cat "${inputs.b_result}" > "${outputs.result}"
            echo "C" >> "${outputs.result}"
          '';
      };
    in
    {
      inherit stageA stageB stageC;
    };

  mkSG =
    {
      hashMode ? "pure",
      runDeps ? [ ],
      parameters ? { },
    }:
    mkScatterGatherStage {
      pname = "sg-test";
      version = "1.0";
      resolvedParameters = parameters;
      inherit hashMode;

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
    };

  allChanged = old: new: (pkgs.lib.intersectLists old new) == [ ];
  noneChanged = old: new: old == new;
  hashOf = stage: stage.jobDirName;
  hashes3 = p: [
    (hashOf p.stageA)
    (hashOf p.stageB)
    (hashOf p.stageC)
  ];

  pureBaseline = mkPipeline { hashMode = "pure"; };
  paramsBaseline = mkPipeline { hashMode = "params-only"; };

  scenarios = {

    pure_vs_params_different = {
      name = "Pure vs params-only produce different hashes";
      assertions = [
        (allChanged (hashes3 pureBaseline) (hashes3 paramsBaseline))
      ];
    };

    params_only_rundep_change_stageA = {
      name = "params-only: runDependency change on stageA has no effect";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            runDepsA = [ pkgs.hello ];
          };
        in
        [
          (noneChanged (hashes3 paramsBaseline) (hashes3 modified))
        ];
    };

    params_only_rundep_change_stageB = {
      name = "params-only: runDependency change on stageB has no effect";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            runDepsB = [ pkgs.hello ];
          };
        in
        [
          (noneChanged (hashes3 paramsBaseline) (hashes3 modified))
        ];
    };

    params_only_rundep_change_stageC = {
      name = "params-only: runDependency change on stageC has no effect";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            runDepsC = [ pkgs.hello ];
          };
        in
        [
          (noneChanged (hashes3 paramsBaseline) (hashes3 modified))
        ];
    };

    params_only_param_change = {
      name = "params-only: parameter change invalidates all stages";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            parameters = {
              x = "changed";
            };
          };
        in
        [
          (allChanged (hashes3 paramsBaseline) (hashes3 modified))
        ];
    };

    params_only_version_change_stageB = {
      name = "params-only: version change on stageB propagates to B and C only";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            versionB = "2.0";
          };
        in
        [
          (noneChanged [ (hashOf paramsBaseline.stageA) ] [ (hashOf modified.stageA) ])
          (allChanged [ (hashOf paramsBaseline.stageB) ] [ (hashOf modified.stageB) ])
          (allChanged [ (hashOf paramsBaseline.stageC) ] [ (hashOf modified.stageC) ])
        ];
    };

    params_only_pname_change_stageC = {
      name = "params-only: pname change on stageC affects only C";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            pnameC = "stage-C-renamed";
          };
        in
        [
          (noneChanged [ (hashOf paramsBaseline.stageA) ] [ (hashOf modified.stageA) ])
          (noneChanged [ (hashOf paramsBaseline.stageB) ] [ (hashOf modified.stageB) ])
          (allChanged [ (hashOf paramsBaseline.stageC) ] [ (hashOf modified.stageC) ])
        ];
    };

    pure_rundep_change_propagates = {
      name = "Pure mode: runDependency change on stageA propagates to all";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "pure";
            runDepsA = [ pkgs.hello ];
          };
        in
        [
          (allChanged [ (hashOf pureBaseline.stageA) ] [ (hashOf modified.stageA) ])
          (allChanged [ (hashOf pureBaseline.stageB) ] [ (hashOf modified.stageB) ])
          (allChanged [ (hashOf pureBaseline.stageC) ] [ (hashOf modified.stageC) ])
        ];
    };

    params_only_sg_rundep_no_change = {
      name = "params-only scatter-gather: runDep change has no effect";
      assertions =
        let
          sgBase = mkSG { hashMode = "params-only"; };
          sgMod = mkSG {
            hashMode = "params-only";
            runDeps = [ pkgs.hello ];
          };
        in
        [
          (noneChanged [ sgBase.jobDirName ] [ sgMod.jobDirName ])
        ];
    };

    pure_sg_rundep_changes = {
      name = "Pure scatter-gather: runDep change affects hash";
      assertions =
        let
          sgBase = mkSG { hashMode = "pure"; };
          sgMod = mkSG {
            hashMode = "pure";
            runDeps = [ pkgs.hello ];
          };
        in
        [
          (allChanged [ sgBase.jobDirName ] [ sgMod.jobDirName ])
        ];
    };

    params_only_sg_param_change = {
      name = "params-only scatter-gather: parameter change affects hash";
      assertions =
        let
          sgBase = mkSG { hashMode = "params-only"; };
          sgMod = mkSG {
            hashMode = "params-only";
            parameters = {
              x = "changed";
            };
          };
        in
        [
          (allChanged [ sgBase.jobDirName ] [ sgMod.jobDirName ])
        ];
    };

    params_only_version_change_stageA = {
      name = "params-only: version change on stageA propagates to all";
      assertions =
        let
          modified = mkPipeline {
            hashMode = "params-only";
            versionA = "2.0";
          };
        in
        [
          (allChanged [ (hashOf paramsBaseline.stageA) ] [ (hashOf modified.stageA) ])
          (allChanged [ (hashOf paramsBaseline.stageB) ] [ (hashOf modified.stageB) ])
          (allChanged [ (hashOf paramsBaseline.stageC) ] [ (hashOf modified.stageC) ])
        ];
    };

    params_only_stability = {
      name = "params-only: identical inputs produce identical hashes";
      assertions =
        let
          second = mkPipeline { hashMode = "params-only"; };
        in
        [
          (noneChanged (hashes3 paramsBaseline) (hashes3 second))
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
pkgs.runCommand "check-hash-mode" { } ''
  echo "Running Hash Mode Tests..."
  ${pkgs.lib.concatMapStringsSep "\n" (msg: "echo '${msg}'") runScenarios}
  echo ""
  echo "All hash mode tests passed."
  touch $out
''
