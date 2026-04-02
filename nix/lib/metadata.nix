{
  pkgs,
  gitHash,
  repxVersion,
  includeImages ? true,
  containerMode ? "none",
  unifiedImage ? null,
  perRunImages ? { },
}:
let
  removeNulls = attrs: pkgs.lib.filterAttrs (_: v: v != null) attrs;

  mkJobMetadata =
    job: stageType: pname: jobNameWithHash:
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

      jobResourceHints = job.resources or null;
    in
    {
      name = jobNameWithHash;
      value = removeNulls {
        params = job.resolvedParameters or { };
        name = job.jobName or null;
        stage_type = stageType;
        executables = addPathToExecutables (job.executables or { });
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

      effectiveImageDrv =
        if containerMode == "unified" then
          unifiedImage
        else if containerMode == "per-run" then
          perRunImages.${runName} or null
        else
          null;
      imagePath =
        if includeImages && effectiveImageDrv != null then
          "images/" + (builtins.baseNameOf (toString effectiveImageDrv))
        else
          null;

      jobsAttrSet = pkgs.lib.listToAttrs (
        map (
          job:
          let
            jobNameWithHash = job.jobDirName;
            inherit (job) pname;
            stageType = job.repxStageType or "simple";
          in
          mkJobMetadata job stageType pname jobNameWithHash
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
    builtins.toFile "metadata-${runName}.json" (
      builtins.unsafeDiscardStringContext (builtins.toJSON metadata)
    );

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
    builtins.toFile "metadata-top.json" (
      builtins.unsafeDiscardStringContext (builtins.toJSON metadata)
    );
in
{
  inherit mkRunMetadata mkRootMetadata;
}
