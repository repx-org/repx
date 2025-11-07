{ pkgs }:
stageDef:
let
  groupPname = stageDef.pname;
  version = stageDef.version or "1.1";
  scatterDef = stageDef.scatter;
  workerDef = stageDef.worker;
  gatherDef = stageDef.gather;
  paramInputs = stageDef.paramInputs or { };

in
if !(scatterDef.outputs ? "worker__arg") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'scatter' section MUST define a special output named "worker__arg".
    This attrset defines the schema of inputs that will be generated for each worker.
    Example: outputs.worker__arg = { startIndex = 0; };
  ''
else if !(scatterDef.outputs ? "work__items") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'scatter' section MUST define an output named "work__items".
    This output specifies the file where the JSON list of work items will be written.
    Example: outputs.work_items = "$out/work_items.json";
  ''
else if !(gatherDef.inputs ? "worker__outs") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'gather' section MUST define an input named "worker__outs".
    This special input receives the manifest of all worker output paths from the runner.
    Example: inputs.worker__outs = "[]";
  ''
else if !(workerDef.inputs ? "worker__item") then
  throw ''
    Scatter-gather stage "${groupPname}" is invalid.
    The 'worker' section MUST define a special input named "worker__item".
    This input receives the single JSON file containing the task for the worker.
    Example: inputs.worker__item = "";
  ''
else
  let
    workerDeclaredInputs = builtins.attrNames (
      pkgs.lib.removeAttrs (workerDef.inputs or { }) [ "worker__item" ]
    );
    stagePassthroughInputs = builtins.attrNames (stageDef.stageInputs or { });
    missingWorkerInputs = pkgs.lib.subtractLists workerDeclaredInputs stagePassthroughInputs;
  in
  if missingWorkerInputs != [ ] then
    throw ''
      Scatter-gather stage "${groupPname}" is invalid.
      The 'worker' stage declares direct file-path inputs that cannot be satisfied.
      These inputs must be provided as dependencies to the overall scatter-gather stage.

      Missing inputs for worker: ${builtins.toJSON missingWorkerInputs}

      Available inputs passed through to the stage: ${builtins.toJSON stagePassthroughInputs}
    ''
  else
    let
      mkSubStage =
        subStageDef: subStageArgs:
        (import ./stage-helper.nix) { inherit pkgs; } (
          subStageDef // subStageArgs // { inherit paramInputs; }
        );

      scatterDrv = mkSubStage scatterDef (
        stageDef
        // {
          pname = "${groupPname}-scatter";
        }
      );
      workerDrv = mkSubStage workerDef {
        pname = "${groupPname}-worker";
      };
      gatherDrv = mkSubStage gatherDef {
        pname = "${groupPname}-gather";
      };

      externalInputMappings = scatterDrv.passthru.executables.main.inputs;
      executables = {
        scatter = {
          inputs = externalInputMappings;
          outputs = scatterDef.outputs or { };
        };

        worker = {
          inputs =
            (pkgs.lib.filter (x: x != null) (
              pkgs.lib.mapAttrsToList (
                targetInput: _:
                let
                  mapping = pkgs.lib.findFirst (m: m.target_input == targetInput) null externalInputMappings;
                in
                if mapping == null then null else mapping
              ) (pkgs.lib.removeAttrs (workerDef.inputs or { }) [ "worker__item" ])
            ))
            ++ [
              {
                source = "scatter:work_item";
                target_input = "worker__item";
              }
            ];

          outputs = workerDef.outputs or { };
        };

        gather = {
          inputs =
            (
              if (gatherDef.inputs ? "worker__outs") then
                let
                  workerOutputNames = builtins.attrNames (workerDef.outputs or { });
                  workerOutputName =
                    if pkgs.lib.length workerOutputNames == 1 then
                      pkgs.lib.head workerOutputNames
                    else
                      throw "A worker stage must define exactly one output. Found: ${toString workerOutputNames}";
                in
                [
                  {
                    source = "runner:worker_outputs";
                    source_key = workerOutputName;
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
        };
      };

      dependencyDerivations = stageDef.dependencyDerivations or [ ];
      depders = pkgs.lib.filter pkgs.lib.isDerivation dependencyDerivations;
      dependencyManifestJson = builtins.toJSON (map (drv: toString drv) depders);

    in
    pkgs.stdenv.mkDerivation rec {
      inherit version;
      pname = groupPname;

      dontUnpack = true;
      nativeBuildInputs = [
        scatterDrv
        workerDrv
        gatherDrv
      ];

      passthru = {
        repxStageType = "scatter-gather";
        inherit paramInputs executables;
        outputMetadata = gatherDef.outputs or { };
      };

      installPhase = ''
        runHook preInstall
        mkdir -p $out/bin
        cp ${scatterDrv}/bin/* $out/bin/${groupPname}-scatter
        cp ${workerDrv}/bin/* $out/bin/${groupPname}-worker
        cp ${gatherDrv}/bin/* $out/bin/${groupPname}-gather
        chmod +x $out/bin/*

        echo '${builtins.toJSON paramInputs}' > $out/${pname}-params.json
        echo '${dependencyManifestJson}' > $out/nix-input-dependencies.json

        runHook postInstall
      '';
    }
