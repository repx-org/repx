args: stageFile: dependencies:
let
  inherit (args) pkgs;
  common = import ./common.nix;
  processDependenciesFn = import ./process-dependencies.nix;
  mkSimpleStage = import ../stage-simple.nix { inherit pkgs; };
  mkScatterGatherStage = import ../stage-scatter-gather.nix { inherit pkgs; };

  stageDef =
    let
      def = pkgs.callPackage stageFile { inherit pkgs; };
      isScatterGather = builtins.hasAttr "scatter" def;
      baseKeys = [
        "pname"
        "version"
        "params"
        "passthru"
        "override"
        "overrideDerivation"
      ];
      simpleStageKeys = baseKeys ++ [
        "inputs"
        "outputs"
        "run"
        "runDependencies"
      ];
      scatterGatherStageKeys = baseKeys ++ [
        "scatter"
        "worker"
        "gather"
        "inputs"
        "runDependencies"
      ];
      validKeys = if isScatterGather then scatterGatherStageKeys else simpleStageKeys;
    in
    common.validateArgs {
      inherit pkgs validKeys;
      name = "Stage definition from file '${toString stageFile}'";
      args = def;
      contextStr = "(Type: ${if isScatterGather then "scatter-gather" else "simple"})";
    };

  consumerInputs =
    if stageDef ? "scatter" then stageDef.scatter.inputs or { } else stageDef.inputs or { };

  processed = processDependenciesFn (
    args
    // {
      inherit dependencies consumerInputs;
      producerPname = stageDef.pname;
    }
  );

  finalResult =
    if !(pkgs.lib.isAttrs stageDef) then
      throw "Stage file '${toString stageFile}' did not return a declarative attribute set."
    else
      let
        declaredParams = stageDef.params or { };
        globalParams = args.paramInputs or { };

        resolvedParams = pkgs.lib.mapAttrs (
          name: default: if builtins.hasAttr name globalParams then globalParams.${name} else default
        ) declaredParams;

        stageDefWithDeps = stageDef // {
          paramInputs = resolvedParams;
          dependencyDerivations = pkgs.lib.unique processed.dependencyDerivations;
          stageInputs = processed.finalFlatInputs;
          inherit (processed) inputMappings;
        };
      in
      if stageDefWithDeps ? "scatter" then
        mkScatterGatherStage stageDefWithDeps
      else
        mkSimpleStage stageDefWithDeps;
in
finalResult
