args: stageFile: dependencies:
let
  inherit (args) pkgs;
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
      actualKeys = builtins.attrNames def;
      invalidKeys = pkgs.lib.subtractLists validKeys actualKeys;
    in
    if invalidKeys != [ ] then
      throw ''
        Stage definition from file '${toString stageFile}' has unknown attributes: ${builtins.toJSON invalidKeys}.
        Valid attributes for a ${
          if isScatterGather then "scatter-gather" else "simple"
        } stage are: ${builtins.toJSON validKeys}.
      ''
    else
      def;

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
        stageDefWithDeps = stageDef // {
          paramInputs = (stageDef.params or { }) // (args.paramInputs or { });
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
