{
  pkgs,
  dependencies,
  consumerInputs,
  producerPname,
  interRunDepTypes ? { },
  dependencyJobs ? { },
  ...
}:
let
  common = import ./common.nix;
  isFirstStage = dependencies == [ ];
  explicitDeps =
    pkgs.lib.foldl'
      (
        acc: item:
        let
          result =
            if common.isVirtualJob item then
              let
                producerJob = item;
                bashOutputs = producerJob.outputMetadata or { };
                validMappings = pkgs.lib.filterAttrs (name: _: pkgs.lib.hasAttr name consumerInputs) bashOutputs;
              in
              if validMappings == { } then
                let
                  producerOutputs = builtins.attrNames bashOutputs;
                  consumerInputNames = builtins.attrNames consumerInputs;
                in
                throw ''
                  Pipeline connection error: Implicit dependency resolution failed.
                  Stage "${producerPname}" depends on "${producerJob.pname}", but they share no matching input/output names.

                  When passing a stage derivation directly (implicit mapping), at least one output name from the producer
                  must match an input name in the consumer.

                  Producer "${producerJob.pname}" outputs: ${builtins.toJSON producerOutputs}
                  Consumer "${producerPname}" inputs:  ${builtins.toJSON consumerInputNames}

                  If the names differ, use the explicit mapping syntax: [ producer "source_output" "target_input" ]
                ''
              else
                let
                  newMappings = pkgs.lib.mapAttrsToList (name: _: {
                    type = "intra-pipeline";
                    job_id = producerJob.jobDirName;
                    source_output = name;
                    target_input = name;
                  }) validMappings;
                  newInputs = pkgs.lib.mapAttrs' (
                    name: _: pkgs.lib.nameValuePair name "\${inputs[\"${name}\"]}"
                  ) validMappings;
                in
                {
                  upstreamJobs = [ item ];
                  finalFlatInputs = newInputs;
                  inputMappings = newMappings;
                }
            else if pkgs.lib.isList item then
              let
                dep = pkgs.lib.head item;
                strings = pkgs.lib.tail item;
                sourceName = pkgs.lib.elemAt strings 0;
                targetName = if pkgs.lib.length strings == 2 then pkgs.lib.elemAt strings 1 else sourceName;
                producerOutputs = dep.outputMetadata or { };
              in
              if !(common.isVirtualJob dep) then
                throw "In [dep, ...], the first element must be a virtual job, but got: ${builtins.typeOf dep}"
              else if !(pkgs.lib.all pkgs.lib.isString strings) then
                throw "In [dep, ...], all elements after the first must be strings."
              else if
                !(pkgs.lib.elem (pkgs.lib.length item) [
                  2
                  3
                ])
              then
                throw "A grouped list dependency must have 2 or 3 elements, but got ${toString (pkgs.lib.length item)}."
              else if !(builtins.hasAttr sourceName producerOutputs) then
                let
                  availableOutputs = builtins.attrNames producerOutputs;
                in
                throw ''
                  Pipeline connection error: Stage validation failed.
                  The producer stage "${dep.pname}" does not have an output named "${sourceName}".
                  Available outputs are: ${builtins.toJSON availableOutputs}
                ''
              else if !(builtins.hasAttr targetName consumerInputs) then
                let
                  availableInputs = builtins.attrNames consumerInputs;
                in
                throw ''
                  Pipeline connection error: Stage validation failed.
                  You are trying to connect to a target input named "${targetName}" on stage "${producerPname}".
                  However, that stage does not declare such an input.
                  Available inputs on "${producerPname}" are: ${builtins.toJSON availableInputs}
                ''
              else
                {
                  upstreamJobs = [ dep ];
                  finalFlatInputs = {
                    ${targetName} = "\${inputs[\"${targetName}\"]}";
                  };
                  inputMappings = [
                    {
                      type = "intra-pipeline";
                      job_id = dep.jobDirName;
                      source_output = sourceName;
                      target_input = targetName;
                    }
                  ];
                }
            else
              throw "Dependency item in '${producerPname}' must be a virtual job or a list. found: ${builtins.typeOf item}";
        in
        {
          upstreamJobs = acc.upstreamJobs ++ result.upstreamJobs;
          finalFlatInputs = pkgs.lib.attrsets.unionOfDisjoint acc.finalFlatInputs result.finalFlatInputs;
          inputMappings = acc.inputMappings ++ result.inputMappings;
        }
      )
      {
        upstreamJobs = [ ];
        finalFlatInputs = { };
        inputMappings = [ ];
      }
      dependencies;

  requiredRunNames = builtins.attrNames interRunDepTypes;

  upstreamInterRunJobs =
    if isFirstStage then
      pkgs.lib.flatten (map (name: dependencyJobs.${name} or [ ]) requiredRunNames)
    else
      [ ];

  implicitMappings =
    if isFirstStage then
      pkgs.lib.concatMap (
        runName:
        let
          metaInput = "metadata__${runName}";
          baseInput = "store__base";
          depType = interRunDepTypes.${runName};
        in
        if !(builtins.hasAttr metaInput consumerInputs) then
          throw ''
            Pipeline Error in stage '${producerPname}':
            This stage is a "First Stage" (it has no intra-pipeline dependencies).
            The Run Definition declares a dependency on run '${runName}'.

            Therefore, this stage MUST accept the input: "${metaInput}".

            Please add '"${metaInput}" = "";' to the inputs of '${producerPname}'.
          ''
        else if !(builtins.hasAttr baseInput consumerInputs) then
          throw ''
            Pipeline Error in stage '${producerPname}':
            This stage is a "First Stage" and the run has external dependencies.
            It MUST accept the input: "store__base".

            Please add '"store__base" = "";' to the inputs of '${producerPname}'.
          ''
        else
          [
            {
              type = "inter-run";
              source_run = runName;
              dependency_type = depType;
              target_input = metaInput;
            }
            {
              type = "global";
              source_value = "store_base";
              target_input = baseInput;
            }
          ]
      ) requiredRunNames
    else
      let
        forbiddenInputs = pkgs.lib.filter (
          name: name == "store__base" || pkgs.lib.hasPrefix "metadata__" name
        ) (builtins.attrNames consumerInputs);
      in
      if forbiddenInputs != [ ] then
        throw ''
          Pipeline Error in stage '${producerPname}':
          This stage depends on other stages within the pipeline. It is NOT a "First Stage".
          Only First Stages are allowed to accept inter-run metadata/store arguments.

          Please remove the following inputs: ${builtins.toJSON forbiddenInputs}

          Pass necessary data from the upstream stages instead.
        ''
      else
        [ ];

  uniqueImplicitMappings = pkgs.lib.unique implicitMappings;

  implicitFlatInputs = pkgs.lib.listToAttrs (
    map (mapping: {
      name = mapping.target_input;
      value = "\${inputs[\"${mapping.target_input}\"]}";
    }) uniqueImplicitMappings
  );

  allSatisfiedInputs = explicitDeps.finalFlatInputs // implicitFlatInputs;
  satisfiedInputNames = builtins.attrNames allSatisfiedInputs;
  requiredInputNames = builtins.attrNames consumerInputs;
  missingInputNames = pkgs.lib.subtractLists satisfiedInputNames requiredInputNames;
in
if missingInputNames != [ ] then
  throw ''
    Pipeline connection error: Unresolved inputs in stage "${producerPname}".
    The following inputs are declared but were not provided by any dependency:
    ${builtins.toJSON missingInputNames}

    Provided inputs: ${builtins.toJSON satisfiedInputNames}
    Declared inputs: ${builtins.toJSON requiredInputNames}

    Please ensure all inputs are mapped from an upstream stage or implicit dependency.
  ''
else
  {
    upstreamJobs = explicitDeps.upstreamJobs ++ upstreamInterRunJobs;
    dependencyDerivations =
      let
        drvsFromJobs = pkgs.lib.concatMap (
          job:
          if job.repxStageType == "scatter-gather" then
            [
              job.scatterDrv
              job.gatherDrv
            ]
            ++ (builtins.attrValues job.stepDrvs)
          else
            [ job.scriptDrv ]
        ) explicitDeps.upstreamJobs;
        interRunDrvs = pkgs.lib.concatMap (
          job:
          if common.isVirtualJob job then
            if job.repxStageType == "scatter-gather" then
              [
                job.scatterDrv
                job.gatherDrv
              ]
              ++ (builtins.attrValues job.stepDrvs)
            else
              [ job.scriptDrv ]
          else
            [ ]
        ) upstreamInterRunJobs;
      in
      common.uniqueDrvs (drvsFromJobs ++ interRunDrvs);
    finalFlatInputs = allSatisfiedInputs;
    inputMappings = explicitDeps.inputMappings ++ uniqueImplicitMappings;
  }
