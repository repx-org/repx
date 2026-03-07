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
        "parameters"
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

  declaredParameters = stageDef.parameters or { };
  globalParameters = args.resolvedParameters or { };
  resolvedParameters = pkgs.lib.mapAttrs (
    name: default: if builtins.hasAttr name globalParameters then globalParameters.${name} else default
  ) declaredParameters;

  resolveWithParameters = common.mkResolveWithParameters resolvedParameters (toString stageFile);

  resolvedPname = resolveWithParameters "pname" (stageDef.pname or (throw "Stage must have a pname"));
  resolvedInputs = resolveWithParameters "inputs" (
    if stageDef ? "scatter" then stageDef.scatter.inputs or { } else stageDef.inputs or { }
  );
  resolvedOutputs = resolveWithParameters "outputs" (stageDef.outputs or { });

  resolvedStageResources = resolveWithParameters "resources" (stageDef.resources or null);

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
    else if !(builtins.isPath stageFile || builtins.isFunction stageFile) then
      throw "call-stage: 'stageFile' must be a path or a function, got ${builtins.typeOf stageFile}."
    else if (stageDef ? "run") && !(builtins.isFunction stageDef.run) then
      throw "Stage '${toString stageFile}': 'run' must be a function, got ${builtins.typeOf stageDef.run}."
    else
      let
        stageDefWithDeps = stageDef // {
          pname = resolvedPname;
          inputs = resolvedInputs;
          outputs = resolvedOutputs;
          inherit resolvedParameters;
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
