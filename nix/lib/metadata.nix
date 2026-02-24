{
  pkgs,
  gitHash,
  repxVersion,
  includeImages ? true,
}:
let
  removeNulls = attrs: pkgs.lib.filterAttrs (_: v: v != null) attrs;

  mkJobMetadata =
    jobDrv: stageType: pname: jobNameWithHash:
    let
      addPathToExecutables = pkgs.lib.mapAttrs (
        exeName: exeDef:
        let
          baseDef = exeDef // {
            path =
              if stageType == "scatter-gather" then
                "jobs/${jobNameWithHash}/bin/${pname}-${exeName}"
              else
                "jobs/${jobNameWithHash}/bin/${pname}";
          };
          withResources =
            if baseDef ? resource_hints && baseDef.resource_hints != null then
              baseDef
            else
              removeAttrs baseDef [ "resource_hints" ];
        in
        withResources
      );

      jobResourceHints = jobDrv.passthru.resources or null;
    in
    {
      name = jobNameWithHash;
      value = removeNulls {
        params = jobDrv.passthru.paramInputs or { };
        name = jobDrv.name or null;
        stage_type = stageType;
        executables = addPathToExecutables (jobDrv.passthru.executables or { });
        resource_hints = jobResourceHints;
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
          "images/" + (builtins.baseNameOf (toString imageDrv))
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
      groups ? { },
    }:
    let
      metadata = {
        repx_version = repxVersion;
        type = "root";
        inherit gitHash;
        runs = runMetadataPaths;
      }
      // (if groups != { } then { inherit groups; } else { });
    in
    pkgs.writeTextFile {
      name = "metadata-top.json";
      text = builtins.toJSON metadata;
    };
in
{
  inherit mkRunMetadata mkRootMetadata;
}
