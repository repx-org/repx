{
  pkgs,
  gitHash,
  includeImages ? true,
}:
runDefinitions: runsAttrSet: jobs:
let
  jobsAttrSet = pkgs.lib.listToAttrs (
    map (
      jobDrv:
      let
        jobNameWithHash = builtins.baseNameOf (builtins.unsafeDiscardStringContext (toString jobDrv));
        pname = jobDrv.pname;
        stageType = jobDrv.passthru.repxStageType or "simple";

        addPathToExecutables = pkgs.lib.mapAttrs (
          exeName: exeDef:
          exeDef
          // {
            path =
              if stageType == "scatter-gather" then
                "jobs/${jobNameWithHash}/bin/${pname}-${exeName}"
              else
                "jobs/${jobNameWithHash}/bin/${pname}";
          }
        );

      in
      {
        name = jobNameWithHash;
        value = {
          params = jobDrv.passthru.paramInputs or { };
          name = jobDrv.name or null;
          stage_type = stageType;

          executables = addPathToExecutables (jobDrv.passthru.executables or { });
        };
      }
    ) jobs
  );

  runsAttrSetForJson =
    let
      findRunDef = runName: pkgs.lib.findFirst (rd: rd.name == runName) null runDefinitions;
    in
    pkgs.lib.mapAttrs (
      runName: jobDerivationsListForRun:
      let
        runDef = findRunDef runName;
        imageDrv = if runDef != null then runDef.image else null;

        imagePath =
          if includeImages && imageDrv != null then
            "image/" + (builtins.baseNameOf (toString imageDrv))
          else
            null;

        jobIds = map (drv: builtins.baseNameOf (toString drv)) (
          pkgs.lib.filter pkgs.lib.isDerivation jobDerivationsListForRun
        );
      in
      {
        image = imagePath;
        jobs = jobIds;
      }
    ) runsAttrSet;

  finalMetadata = {
    schema_version = "1.0";
    inherit gitHash;
    runs = runsAttrSetForJson;
    jobs = jobsAttrSet;
  };

in
pkgs.writeTextFile {
  name = "experiment-metadata-json";
  destination = "/metadata.json";
  text = builtins.toJSON finalMetadata;
}
