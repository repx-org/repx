{ pkgs }:
stageDef:
let
  common = import ./internal/common.nix;
  groupPname = stageDef.pname;
  version = stageDef.version or "1.1";

  subStageKeys = [
    "pname"
    "inputs"
    "outputs"
    "run"
    "runDependencies"
    "resources"
    "deps"
  ];

  scatterGatherSubKeys = [
    "pname"
    "inputs"
    "outputs"
    "run"
    "runDependencies"
    "resources"
  ];

  validateSubStage =
    name: validKeys: args:
    common.validateArgs {
      inherit pkgs args;
      name = stageDef.pname;
      inherit validKeys;
      contextStr = "in '${name}' definition of scatter-gather stage";
    };

  scatterDef = validateSubStage "scatter" scatterGatherSubKeys stageDef.scatter;
  gatherDef = validateSubStage "gather" scatterGatherSubKeys stageDef.gather;

  stepsAttrs = stageDef.steps;
  stepNames = builtins.attrNames stepsAttrs;
  stepsDefs = pkgs.lib.mapAttrs (
    name: def: validateSubStage "step '${name}'" subStageKeys def
  ) stepsAttrs;

  allStepDeps = pkgs.lib.mapAttrs (_: def: def.deps or [ ]) stepsDefs;

  getDepName =
    dep:
    if pkgs.lib.isList dep then
      let
        depRef = pkgs.lib.head dep;
      in
      pkgs.lib.findFirst (
        name: stepsAttrs.${name} == depRef
      ) (throw "Step dep list references an unknown step") stepNames
    else
      pkgs.lib.findFirst (
        name: stepsAttrs.${name} == dep
      ) (throw "Step dep references an unknown step") stepNames;

  stepDepNames = pkgs.lib.mapAttrs (_: deps: map getDepName deps) allStepDeps;

  dependedUpon = pkgs.lib.unique (pkgs.lib.concatLists (builtins.attrValues stepDepNames));

  rootStepNames = builtins.filter (name: stepDepNames.${name} == [ ]) stepNames;

  sinkStepNames = pkgs.lib.subtractLists dependedUpon stepNames;

  resolvedParameters = stageDef.resolvedParameters or { };

  mkSubStage =
    subStageDef: subStageArgs:
    (import ./stage-simple.nix) { inherit pkgs; } (
      (pkgs.lib.removeAttrs subStageDef [ "deps" ])
      // subStageArgs
      // {
        dependencyDerivations = [ ];
        inherit resolvedParameters;
      }
    );

  commonStageDef = pkgs.lib.removeAttrs stageDef [
    "inputs"
    "outputs"
    "pname"
    "scatter"
    "steps"
    "gather"
  ];

  scatterSubJob = mkSubStage scatterDef (
    commonStageDef
    // {
      pname = "${groupPname}-scatter";
    }
  );

  stepSubJobs = pkgs.lib.mapAttrs (
    name: def:
    mkSubStage def {
      pname = "${groupPname}-step-${name}";
    }
  ) stepsDefs;

  gatherSubJob = mkSubStage gatherDef {
    pname = "${groupPname}-gather";
  };

  scatterDrv = scatterSubJob.scriptDrv;
  stepDrvs = pkgs.lib.mapAttrs (_: subJob: subJob.scriptDrv) stepSubJobs;
  gatherDrv = gatherSubJob.scriptDrv;

  externalInputMappings = scatterSubJob.templateData.executables.main.inputs;

  scatterResources = common.validateResourceHints {
    inherit pkgs;
    resources = scatterDef.resources or null;
    contextStr = "scatter-gather stage '${groupPname}', scatter resources";
  };
  gatherResources = common.validateResourceHints {
    inherit pkgs;
    resources = gatherDef.resources or null;
    contextStr = "scatter-gather stage '${groupPname}', gather resources";
  };

  resolveStepInputMappings =
    stepName: stepDef:
    let
      deps = stepDef.deps or [ ];
      consumerInputs = stepDef.inputs or { };

      resolvedDeps = map (
        dep:
        if pkgs.lib.isList dep then
          let
            strings = pkgs.lib.tail dep;
            depName = getDepName dep;
            sourceName = pkgs.lib.elemAt strings 0;
            targetName = if pkgs.lib.length strings >= 2 then pkgs.lib.elemAt strings 1 else sourceName;
            producerOutputs = stepsDefs.${depName}.outputs or { };
          in
          if !(builtins.hasAttr sourceName producerOutputs) then
            throw ''
              Scatter-gather stage "${groupPname}", step "${stepName}":
              Dependency step "${depName}" does not have output "${sourceName}".
              Available outputs: ${builtins.toJSON (builtins.attrNames producerOutputs)}
            ''
          else if !(builtins.hasAttr targetName consumerInputs) then
            throw ''
              Scatter-gather stage "${groupPname}", step "${stepName}":
              Explicit mapping targets input "${targetName}", but step does not declare it.
              Available inputs: ${builtins.toJSON (builtins.attrNames consumerInputs)}
            ''
          else
            [
              {
                source = "step:${depName}";
                source_output = sourceName;
                target_input = targetName;
              }
            ]
        else
          let
            depName = getDepName dep;
            producerOutputs = stepsDefs.${depName}.outputs or { };
            matchingNames = pkgs.lib.intersectLists (builtins.attrNames producerOutputs) (
              builtins.attrNames consumerInputs
            );
          in
          if matchingNames == [ ] then
            throw ''
              Scatter-gather stage "${groupPname}", step "${stepName}":
              Implicit dependency on step "${depName}" found no matching input/output names.
              Step "${depName}" outputs: ${builtins.toJSON (builtins.attrNames producerOutputs)}
              Step "${stepName}" inputs: ${builtins.toJSON (builtins.attrNames consumerInputs)}
              Use explicit mapping: [ ${depName} "source_output" "target_input" ]
            ''
          else
            map (name: {
              source = "step:${depName}";
              source_output = name;
              target_input = name;
            }) matchingNames
      ) deps;

      stepInputMappings = pkgs.lib.concatLists resolvedDeps;

      satisfiedBySteps = map (m: m.target_input) stepInputMappings;

      remainingInputNames = pkgs.lib.subtractLists satisfiedBySteps (builtins.attrNames consumerInputs);

      externalMappings = pkgs.lib.concatMap (
        inputName:
        if inputName == "worker__item" then
          [
            {
              source = "scatter:work_item";
              target_input = "worker__item";
            }
          ]
        else
          let
            mapping = pkgs.lib.findFirst (m: m.target_input == inputName) null externalInputMappings;
          in
          if mapping != null then [ mapping ] else [ ]
      ) remainingInputNames;

      allMappings = stepInputMappings ++ externalMappings;

      satisfiedInputs = map (m: m.target_input) allMappings;
      unsatisfiedInputs = pkgs.lib.subtractLists satisfiedInputs (builtins.attrNames consumerInputs);
    in
    if unsatisfiedInputs != [ ] then
      throw ''
        Scatter-gather stage "${groupPname}", step "${stepName}":
        The following inputs are not satisfied by any step dependency or external input:
        ${builtins.toJSON unsatisfiedInputs}

        Satisfied inputs: ${builtins.toJSON satisfiedInputs}
        Declared inputs: ${builtins.toJSON (builtins.attrNames consumerInputs)}
      ''
    else
      allMappings;

  sinkStepName = builtins.head sinkStepNames;
  sinkStepDef = stepsDefs.${sinkStepName};

  stepExecutables = pkgs.lib.mapAttrs' (stepName: stepDef: {
    name = "step-${stepName}";
    value = {
      inputs = resolveStepInputMappings stepName stepDef;
      outputs = stepDef.outputs or { };
      resource_hints = common.validateResourceHints {
        inherit pkgs;
        resources = stepDef.resources or null;
        contextStr = "scatter-gather stage '${groupPname}', step '${stepName}' resources";
      };
      deps = stepDepNames.${stepName};
    };
  }) stepsDefs;

  executables = {
    scatter = {
      inputs = externalInputMappings;
      outputs = scatterDef.outputs or { };
      resource_hints = scatterResources;
    };
  }
  // stepExecutables
  // {
    gather = {
      inputs =
        (
          if (gatherDef.inputs ? "worker__outs") then
            let
              sinkOutputNames = builtins.attrNames (sinkStepDef.outputs or { });
              sinkOutputName =
                if pkgs.lib.length sinkOutputNames == 1 then
                  pkgs.lib.head sinkOutputNames
                else
                  throw ''
                    Scatter-gather stage "${groupPname}":
                    The sink step "${sinkStepName}" must define exactly one output for gather.
                    Found: ${builtins.toJSON sinkOutputNames}
                  '';
            in
            [
              {
                source = "runner:worker_outputs";
                source_key = sinkOutputName;
                target_input = "worker__outs";
              }
            ]
          else
            [ ]
        )
        ++ (
          let
            scatterRegularOutputs = builtins.attrNames (
              pkgs.lib.removeAttrs (scatterDef.outputs or { }) [
                "worker__arg"
                "work__items"
              ]
            );
            gatherRegularInputs = builtins.attrNames (
              pkgs.lib.removeAttrs (gatherDef.inputs or { }) [ "worker__outs" ]
            );
            scatterInputsForGather = pkgs.lib.intersectLists scatterRegularOutputs gatherRegularInputs;
          in
          map (inputName: {
            job_id = "self";
            source_output = inputName;
            target_input = inputName;
          }) scatterInputsForGather
        );

      outputs = gatherDef.outputs or { };
      resource_hints = gatherResources;
    };
  };

  rootStepsWithWorkerItem = builtins.filter (
    name: stepsDefs.${name}.inputs ? "worker__item"
  ) rootStepNames;

in
if stepNames == [ ] then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'steps' attrset must contain at least one step definition.
  ''
else if !(scatterDef.outputs ? "worker__arg") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'scatter' section MUST define a special output named "worker__arg".
  ''
else if !(scatterDef.outputs ? "work__items") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'scatter' section MUST define an output named "work__items".
  ''
else if !(gatherDef.inputs ? "worker__outs") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'gather' section MUST define an input named "worker__outs".
  ''
else if rootStepNames == [ ] then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    No root steps found (every step has deps). There must be a cycle.
  ''
else if builtins.length sinkStepNames != 1 then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    There must be exactly one sink step.
    Found ${toString (builtins.length sinkStepNames)}: ${builtins.toJSON sinkStepNames}
  ''
else
  assert
    rootStepsWithWorkerItem != [ ]
    || throw ''
      Scatter-gather stage "${groupPname}" is invalid.
      At least one root step must declare a "worker__item" input.
      Root steps: ${builtins.toJSON rootStepNames}
    '';
  {
    _repx_virtual_job = true;
    inherit executables;
    pname = groupPname;
    inherit version;
    repxStageType = "scatter-gather";
    outputMetadata = gatherDef.outputs or { };
    inherit scatterDrv gatherDrv stepDrvs;
    resources = stageDef.resources or null;
    inputMappings = stageDef.inputMappings or [ ];
    scriptDrv = null;

    templateData = {
      pname = groupPname;
      inherit version;
      stage_type = "scatter-gather";
      scatter_drv = builtins.unsafeDiscardStringContext (toString scatterDrv);
      gather_drv = builtins.unsafeDiscardStringContext (toString gatherDrv);
      step_drvs = pkgs.lib.mapAttrs (_: drv: builtins.unsafeDiscardStringContext (toString drv)) stepDrvs;
      inherit executables;
      step_deps = stepDepNames;
      outputs = gatherDef.outputs or { };
      input_mappings = stageDef.inputMappings or [ ];
      resources = common.validateResourceHints {
        inherit pkgs;
        resources = stageDef.resources or null;
        contextStr = "scatter-gather stage '${groupPname}' resources";
      };
    };
  }
