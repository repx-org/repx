{
  pkgs,
  dependencies,
  consumerInputs,
  producerPname,
  interRunDepTypes ? { },
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

                  Producer "${producerJob.pname}" outputs: ${builtins.toJSON producerOutputs}
                  Consumer "${producerPname}" inputs:  ${builtins.toJSON consumerInputNames}

                  Use the explicit mapping syntax: [ producer "source_output" "target_input" ]
                ''
              else
                let
                  newMappings = pkgs.lib.mapAttrsToList (name: _: {
                    type = "intra-pipeline";
                    job_id_template = producerJob.pname;
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
                throw "In [dep, ...], the first element must be a virtual job."
              else if !(pkgs.lib.all pkgs.lib.isString strings) then
                throw "In [dep, ...], all elements after the first must be strings."
              else if
                !(pkgs.lib.elem (pkgs.lib.length item) [
                  2
                  3
                ])
              then
                throw "A grouped list dependency must have 2 or 3 elements."
              else if !(builtins.hasAttr sourceName producerOutputs) then
                throw ''
                  Pipeline connection error: Stage "${dep.pname}" does not have output "${sourceName}".
                  Available outputs: ${builtins.toJSON (builtins.attrNames producerOutputs)}
                ''
              else if !(builtins.hasAttr targetName consumerInputs) then
                throw ''
                  Pipeline connection error: Stage "${producerPname}" does not have input "${targetName}".
                  Available inputs: ${builtins.toJSON (builtins.attrNames consumerInputs)}
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
                      job_id_template = dep.pname;
                      source_output = sourceName;
                      target_input = targetName;
                    }
                  ];
                }
            else
              throw "Dependency in '${producerPname}' must be a virtual job or a list. Got: ${builtins.typeOf item}";
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
            First stage must accept input: "${metaInput}".
          ''
        else if !(builtins.hasAttr baseInput consumerInputs) then
          throw ''
            Pipeline Error in stage '${producerPname}':
            First stage with external deps must accept input: "store__base".
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
          Non-first stages cannot accept inter-run inputs: ${builtins.toJSON forbiddenInputs}
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
    Missing: ${builtins.toJSON missingInputNames}
    Provided: ${builtins.toJSON satisfiedInputNames}
    Declared: ${builtins.toJSON requiredInputNames}
  ''
else
  {
    inherit (explicitDeps) upstreamJobs;
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
      in
      common.uniqueDrvs drvsFromJobs;
    finalFlatInputs = allSatisfiedInputs;
    inputMappings = explicitDeps.inputMappings ++ uniqueImplicitMappings;
  }
