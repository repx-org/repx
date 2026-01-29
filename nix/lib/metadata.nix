{
  pkgs,
  gitHash,
  includeImages ? true,
}:
let
  mkJobMetadata =
    jobDrv: stageType: pname: jobNameWithHash:
    let
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
    };

  mkRunMetadata =
    {
      runDef,
      jobs,
      resolvedDependencies,
    }:
    let
      runName = runDef.name;

      imageDrv = runDef.image;
      imagePath =
        if includeImages && imageDrv != null then
          "image/" + (builtins.baseNameOf (toString imageDrv))
        else
          null;

      jobsAttrSet = pkgs.lib.listToAttrs (
        map (
          jobDrv:
          let
            jobNameWithHash = builtins.baseNameOf (builtins.unsafeDiscardStringContext (toString jobDrv));
            inherit (jobDrv) pname;
            stageType = jobDrv.passthru.repxStageType or "simple";
          in
          mkJobMetadata jobDrv stageType pname jobNameWithHash
        ) jobs
      );

      metadata = {
        schema_version = "1.0";
        type = "run";
        name = runName;
        inherit gitHash;
        dependencies = resolvedDependencies;
        image = imagePath;
        jobs = jobsAttrSet;
      };
    in
    pkgs.writeTextFile {
      name = "metadata-${runName}.json";
      text = builtins.toJSON metadata;
    };

  mkRootMetadata =
    {
      runMetadataPaths,
    }:
    let
      metadata = {
        schema_version = "1.0";
        type = "root";
        inherit gitHash;
        runs = runMetadataPaths;
      };
    in
    pkgs.writeTextFile {
      name = "metadata-top.json";
      text = builtins.toJSON metadata;
    };
in
{
  inherit mkRunMetadata mkRootMetadata;
}
