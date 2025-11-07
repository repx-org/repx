let
  mkSimpleStageInternal = import ./stage-helper.nix;
  mkScatterGatherStageInternal = import ./stage-scatter-gather-helper.nix;

  mkPipelineHelpers =
    { pkgs, repx-lib, ... }@args:
    let
      callStageImpl =
        stageFile: dependencies:
        let
          stageDef = pkgs.callPackage stageFile { inherit pkgs; };

          consumerInputs =
            if stageDef ? "scatter" then stageDef.scatter.inputs or { } else stageDef.inputs or { };

          processed =
            pkgs.lib.foldl'
              (
                acc: item:
                let
                  resolveDep =
                    depItem:
                    if pkgs.lib.isDerivation depItem then
                      {
                        drv = depItem;
                        pname = depItem.pname;
                      }
                    else
                      throw "Unsupported dependency type: ${toString depItem}";
                  processResolvedDep =
                    {
                      dep,
                      drv,
                      pname,
                      sourceName,
                      targetName,
                    }:
                    let
                      inputValue = "\${inputs[\"${targetName}\"]}";
                      newInput = {
                        ${targetName} = inputValue;
                      };
                      newMapping = {
                        job_id = builtins.baseNameOf (toString drv);
                        source_output = sourceName;
                        target_input = targetName;
                      };
                      depDrvs = [ dep ];
                    in
                    acc
                    // {
                      dependencyDerivations = acc.dependencyDerivations ++ depDrvs;
                      finalFlatInputs = pkgs.lib.attrsets.unionOfDisjoint acc.finalFlatInputs newInput;
                      inputMappings = acc.inputMappings ++ [ newMapping ];
                    };
                in
                if pkgs.lib.isDerivation item then
                  let
                    producerDrv = item;
                    bashOutputs = producerDrv.passthru.outputMetadata or { };
                    validMappings = pkgs.lib.filterAttrs (name: _: pkgs.lib.hasAttr name consumerInputs) bashOutputs;
                    newMappings = pkgs.lib.mapAttrsToList (name: _: {
                      job_id = builtins.baseNameOf (toString producerDrv);
                      source_output = name;
                      target_input = name;
                    }) validMappings;
                    newInputs = pkgs.lib.mapAttrs' (
                      name: _: pkgs.lib.nameValuePair name "\${inputs[\"${name}\"]}"
                    ) validMappings;
                    depDrvs = [ item ];
                  in
                  acc
                  // {
                    dependencyDerivations = acc.dependencyDerivations ++ depDrvs;
                    finalFlatInputs = pkgs.lib.attrsets.unionOfDisjoint acc.finalFlatInputs newInputs;
                    inputMappings = acc.inputMappings ++ newMappings;
                  }
                else if pkgs.lib.isList item then
                  let
                    dep = pkgs.lib.head item;
                    strings = pkgs.lib.tail item;
                    sourceName = pkgs.lib.elemAt strings 0;
                    targetName = if pkgs.lib.length strings == 2 then pkgs.lib.elemAt strings 1 else sourceName;
                    resolved = resolveDep dep;
                    producerOutputs = resolved.drv.passthru.outputMetadata or { };
                  in
                  if !(pkgs.lib.isDerivation dep) then
                    throw "In [dep, ...], the first element must be a derivation, but got: ${toString dep}"
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
                      The producer stage "${resolved.pname}" does not have an output named "${sourceName}".
                      Available outputs are: ${builtins.toJSON availableOutputs}
                    ''
                  else if !(builtins.hasAttr targetName consumerInputs) then
                    let
                      availableInputs = builtins.attrNames consumerInputs;
                    in
                    throw ''
                      Pipeline connection error: Stage validation failed.
                      You are trying to connect to a target input named "${targetName}" on stage "${stageDef.pname}".
                      However, that stage does not declare such an input.
                      Available inputs on "${stageDef.pname}" are: ${builtins.toJSON availableInputs}
                    ''
                  else
                    processResolvedDep {
                      inherit dep;
                      drv = resolved.drv;
                      pname = resolved.pname;
                      inherit sourceName targetName;
                    }
                else if pkgs.lib.isAttrs item then
                  let
                    requiredKeys = [
                      "fromRun"
                      "outputName"
                      "targetInput"
                    ];
                    missingKeys = pkgs.lib.filter (k: !pkgs.lib.hasAttr k item) requiredKeys;
                  in
                  if missingKeys != [ ] then
                    throw "A run collection dependency is missing required keys: ${toString missingKeys}"
                  else
                    let
                      runDef = item.fromRun;
                      outputName = item.outputName;
                      targetInput = item.targetInput;
                      stageFilter = item.filterByStage or null;
                      allJobsInRun = pkgs.lib.unique (
                        pkgs.lib.filter pkgs.lib.isDerivation (pkgs.lib.flatten (map pkgs.lib.attrValues runDef.runs))
                      );
                      filteredJobs =
                        if stageFilter != null then
                          pkgs.lib.filter (job: job.pname == stageFilter) allJobsInRun
                        else
                          allJobsInRun;
                      manifestEntries = map (
                        job:
                        let
                          outputPathTemplate =
                            job.passthru.outputMetadata.${outputName}
                              or (throw "Output '${outputName}' not found in job '${job.pname}'");
                          jobId = builtins.baseNameOf (toString job);
                        in
                        {
                          params = job.passthru.paramInputs or { };
                          job_id = jobId;
                          output_template = outputPathTemplate;
                        }
                      ) filteredJobs;
                      collectionManifestFile = pkgs.writeText "${targetInput}-manifest.json" (
                        builtins.toJSON manifestEntries
                      );
                      newInput = {
                        ${targetInput} = toString collectionManifestFile;
                      };
                    in
                    acc
                    // {
                      dependencyDerivations = acc.dependencyDerivations ++ [ collectionManifestFile ];
                      finalFlatInputs = pkgs.lib.attrsets.unionOfDisjoint acc.finalFlatInputs newInput;
                      inputMappings = acc.inputMappings;
                    }
                else
                  throw "Dependency item must be a derivation, a list, or an attrset, but got: ${toString item}"
              )
              {
                dependencyDerivations = [ ];
                finalFlatInputs = { };
                inputMappings = [ ];
              }
              dependencies;

          finalResult =
            if !(pkgs.lib.isAttrs stageDef) then
              throw "Stage file '${toString stageFile}' did not return a declarative attribute set. The old functional API is no longer supported."
            else
              let
                stageDefWithDeps = stageDef // {
                  paramInputs = (stageDef.params or { }) // (args.paramInputs or { });
                  dependencyDerivations = pkgs.lib.unique processed.dependencyDerivations;
                  stageInputs = processed.finalFlatInputs;
                  inputMappings = processed.inputMappings;
                };
              in
              if stageDefWithDeps ? "scatter" then
                mkScatterGatherStageInternal { inherit pkgs; } stageDefWithDeps
              else
                mkSimpleStageInternal { inherit pkgs; } stageDefWithDeps;
        in
        finalResult;
    in
    {
      mkPipe = stages: stages;
      callStage =
        stageFile: dependencies:
        callStageImpl stageFile (if dependencies == null then [ ] else dependencies);
    };

  mkRun =
    {
      pkgs,
      repx-lib,
      name,
      containerized ? true,
      pipelines,
      params,
      paramsDependencies ? [ ],
    }:
    let
      allParams = params // {
        pipeline = pipelines;
      };
      allCombinations = pkgs.lib.cartesianProduct allParams;

      repxForDiscovery = repx-lib.mkPipelineHelpers {
        inherit pkgs repx-lib;
      };

      getDrvsFromPipeline =
        pipeline:
        pkgs.lib.flatten (
          pkgs.lib.map (stageResult: if pkgs.lib.isDerivation stageResult then stageResult else [ ]) (
            pkgs.lib.attrValues pipeline
          )
        );

      loadedPipelines = pkgs.lib.map (
        p:
        pkgs.callPackage p {
          repx = repxForDiscovery;
        }
      ) pipelines;
    in
    {
      inherit name;

      image =
        if containerized then
          pkgs.dockerTools.buildLayeredImage {
            name = name + "-image";
            tag = "latest";
            contents =
              (pkgs.lib.flatten (map getDrvsFromPipeline loadedPipelines))
              ++ [
                pkgs.jq
                pkgs.bash
                pkgs.coreutils
                pkgs.findutils
                pkgs.gnused
                pkgs.gawk
                pkgs.gnugrep
              ]
              ++ paramsDependencies;
            config = {
              Entrypoint = [ "${pkgs.bash}/bin/bash" ];
            };
          }
        else
          null;

      runs = pkgs.lib.map (
        combo:
        let
          pipelinePath = combo.pipeline;
          paramInputs = pkgs.lib.removeAttrs combo [ "pipeline" ];
          repxForPipeline = repx-lib.mkPipelineHelpers {
            inherit pkgs repx-lib paramInputs;
          };
        in
        pkgs.callPackage pipelinePath {
          repx = repxForPipeline;
        }
      ) allCombinations;
    };
in
{
  inherit mkRun mkPipelineHelpers;
  mkLab = import ./lab.nix;
}
