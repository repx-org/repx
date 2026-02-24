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

  paramInputs = stageDef.paramInputs or { };

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
    Example: outputs.worker__arg = { startIndex = 0; };
  ''
else if !(scatterDef.outputs ? "work__items") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'scatter' section MUST define an output named "work__items".
    Example: outputs.work__items = "$out/work_items.json";
  ''
else if !(gatherDef.inputs ? "worker__outs") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'gather' section MUST define an input named "worker__outs".
    Example: inputs.worker__outs = "[]";
  ''
else if rootStepNames == [ ] then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    No root steps found (every step has deps). There must be a cycle.
  ''
else if builtins.length sinkStepNames != 1 then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    There must be exactly one sink step (a step that no other step depends on).
    Found ${toString (builtins.length sinkStepNames)} sink steps: ${builtins.toJSON sinkStepNames}
    The sink step's outputs are what the gather receives.
  ''
else
  let
    sinkStepName = builtins.head sinkStepNames;
    sinkStepDef = stepsDefs.${sinkStepName};

    rootStepsWithWorkerItem = builtins.filter (
      name: stepsDefs.${name}.inputs ? "worker__item"
    ) rootStepNames;

    mkSubStage =
      subStageDef: subStageArgs:
      (import ./stage-simple.nix) { inherit pkgs; } (
        (pkgs.lib.removeAttrs subStageDef [ "deps" ])
        // subStageArgs
        // {
          inherit paramInputs;
          dependencyDerivations = [ ];
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

    scatterDrv = mkSubStage scatterDef (
      commonStageDef
      // {
        pname = "${groupPname}-scatter";
      }
    );

    stepDrvs = pkgs.lib.mapAttrs (
      name: def:
      mkSubStage def {
        pname = "${groupPname}-step-${name}";
      }
    ) stepsDefs;

    gatherDrv = mkSubStage gatherDef {
      pname = "${groupPname}-gather";
    };

    externalInputMappings = scatterDrv.passthru.executables.main.inputs;

    scatterResources = scatterDef.resources or null;
    gatherResources = gatherDef.resources or null;

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

    stepExecutables = pkgs.lib.mapAttrs' (stepName: stepDef: {
      name = "step-${stepName}";
      value = {
        inputs = resolveStepInputMappings stepName stepDef;
        outputs = stepDef.outputs or { };
        resource_hints = stepDef.resources or null;
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

    dependencyDerivations = stageDef.dependencyDerivations or [ ];
    depders = dependencyDerivations;
    dependencyPaths = map toString depders;
    dependencyManifestJson = builtins.toJSON (map builtins.unsafeDiscardStringContext dependencyPaths);
    dependencyHash = builtins.hashString "sha256" (builtins.concatStringsSep ":" dependencyPaths);
    paramsJson = builtins.toJSON paramInputs;

    stepDrvsList = builtins.attrValues stepDrvs;

  in
  assert
    rootStepsWithWorkerItem != [ ]
    || throw ''
      Scatter-gather stage "${groupPname}" is invalid.
      At least one root step (a step with no deps) must declare a "worker__item" input
      to receive the work item from the scatter phase.
      Root steps: ${builtins.toJSON rootStepNames}
    '';
  pkgs.stdenv.mkDerivation rec {
    inherit version;
    pname = groupPname;

    dontUnpack = true;

    nativeBuildInputs = [
      scatterDrv
    ]
    ++ stepDrvsList
    ++ [
      gatherDrv
    ];

    passthru = {
      repxStageType = "scatter-gather";
      inherit paramInputs executables;
      outputMetadata = gatherDef.outputs or { };
      inherit scatterDrv gatherDrv stepDrvs;
      resources = stageDef.resources or null;
    };

    inherit paramsJson dependencyManifestJson dependencyHash;
    passAsFile = [
      "paramsJson"
      "dependencyManifestJson"
    ];

    installPhase =
      let
        stepCopyCommands = pkgs.lib.concatStrings (
          pkgs.lib.mapAttrsToList (
            name: drv: "cp ${drv}/bin/* $out/bin/${groupPname}-step-${name}\n"
          ) stepDrvs
        );
      in
      ''
        runHook preInstall
        mkdir -p $out/bin
        cp ${scatterDrv}/bin/* $out/bin/${groupPname}-scatter
        ${stepCopyCommands}cp ${gatherDrv}/bin/* $out/bin/${groupPname}-gather
        chmod +x $out/bin/*

        cp "$paramsJsonPath" $out/${pname}-params.json
        cp "$dependencyManifestJsonPath" $out/nix-input-dependencies.json

        runHook postInstall
      '';
  }
