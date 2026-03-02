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
        "resources"
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
        "steps"
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

  declaredParams = stageDef.params or { };
  globalParams = args.paramInputs or { };
  resolvedParams = pkgs.lib.mapAttrs (
    name: default: if builtins.hasAttr name globalParams then globalParams.${name} else default
  ) declaredParams;

  resolveWithParams = common.mkResolveWithParams resolvedParams (toString stageFile);

  resolvedPname = resolveWithParams "pname" (stageDef.pname or (throw "Stage must have a pname"));
  resolvedInputs = resolveWithParams "inputs" (
    if stageDef ? "scatter" then stageDef.scatter.inputs or { } else stageDef.inputs or { }
  );
  resolvedOutputs = resolveWithParams "outputs" (stageDef.outputs or { });

  resolvedStageResources = resolveWithParams "resources" (stageDef.resources or null);

  finalResources = common.validateResourceHints {
    inherit pkgs;
    resources = resolvedStageResources;
    contextStr = "stage '${resolvedPname}' resources";
  };

  processed = processDependenciesFn (
    args
    // {
      inherit dependencies;
      consumerInputs = resolvedInputs;
      producerPname = resolvedPname;
    }
  );

  finalResult =
    if !(pkgs.lib.isAttrs stageDef) then
      throw "Stage file '${toString stageFile}' did not return a declarative attribute set."
    else
      let
        stageDefWithDeps = stageDef // {
          pname = resolvedPname;
          inputs = resolvedInputs;
          outputs = resolvedOutputs;
          paramInputs = resolvedParams;
          dependencyDerivations = common.uniqueDrvs processed.dependencyDerivations;
          stageInputs = processed.finalFlatInputs;
          inherit (processed) inputMappings;
          resources = if finalResources == { } then null else finalResources;
        };
      in
      if stageDefWithDeps ? "scatter" then
        mkScatterGatherStage stageDefWithDeps
      else
        mkSimpleStage stageDefWithDeps;
in
finalResult
