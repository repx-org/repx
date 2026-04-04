{
  pkgs,
  repx-lib,
  name,
  pipelines,
  parameters,
  parametersDependencies ? [ ],
  interRunDepTypes ? { },
  ...
}@args:
let
  validKeys = [
    "pkgs"
    "repx-lib"
    "name"
    "pipelines"
    "parameters"
    "parametersDependencies"
    "dependencyJobs"
    "interRunDepTypes"
    "hashMode"
    "override"
    "overrideDerivation"
  ];

  hashMode = args.hashMode or "pure";
  validHashModes = [
    "pure"
    "params-only"
  ];

  actualKeys = builtins.attrNames args;
  invalidKeys = pkgs.lib.subtractLists validKeys actualKeys;

  common = import ./common.nix;

  zipGroupEntries = pkgs.lib.filterAttrs (
    _: val: builtins.isAttrs val && (val._repx_zip or false)
  ) parameters;

  normalParameters = pkgs.lib.filterAttrs (
    _: val: !(builtins.isAttrs val && (val._repx_zip or false))
  ) parameters;

  normalParameterNames = builtins.attrNames normalParameters;

  allZipMembers = pkgs.lib.concatLists (
    pkgs.lib.mapAttrsToList (
      anchor: zipGroup:
      map (member: {
        inherit anchor member;
      }) (builtins.attrNames zipGroup.groups)
    ) zipGroupEntries
  );

  zipVsNormalCollisions = builtins.filter (
    m: builtins.elem m.member normalParameterNames
  ) allZipMembers;

  zipVsZipCollisions =
    let
      byName = builtins.groupBy (m: m.member) allZipMembers;
    in
    pkgs.lib.filterAttrs (_: entries: builtins.length entries > 1) byName;

  anchorVsMemberCollisions = pkgs.lib.filter (m: m.anchor == m.member) allZipMembers;

  zipCollisionAsserts =
    if zipVsNormalCollisions != [ ] then
      let
        first = builtins.head zipVsNormalCollisions;
      in
      throw ''
        Parameter collision in run "${name}".
        '${first.member}' is defined both as a normal parameter and inside utils.zip (anchor '${first.anchor}').
      ''
    else if zipVsZipCollisions != { } then
      let
        collName = builtins.head (builtins.attrNames zipVsZipCollisions);
        entries = zipVsZipCollisions.${collName};
        anchors = pkgs.lib.concatStringsSep ", " (map (e: "'${e.anchor}'") entries);
      in
      throw ''
        Parameter collision in run "${name}".
        '${collName}' appears in multiple utils.zip groups: ${anchors}.
      ''
    else if anchorVsMemberCollisions != [ ] then
      let
        first = builtins.head anchorVsMemberCollisions;
      in
      throw ''
        Parameter collision in run "${name}".
        '${first.anchor}' is used as both the zip anchor key and a member name inside it.
      ''
    else
      true;

  processedParameters = pkgs.lib.mapAttrs (
    _: val:
    if (builtins.isAttrs val) && (val._repx_param or false) then
      {
        inherit (val) values;
        context = val.context or [ ];
      }
    else
      {
        values =
          if builtins.isPath val then
            builtins.path {
              path = val;
              name = baseNameOf val;
            }
          else
            val;
        context = [ ];
      }
  ) normalParameters;

  parameterAxes = pkgs.lib.mapAttrs (_: p: p.values) processedParameters;

  smartParameterContext = pkgs.lib.flatten (
    pkgs.lib.mapAttrsToList (_: p: p.context) processedParameters
  );

  zipGroupsList = pkgs.lib.mapAttrsToList (
    _anchor: zipGroup:
    let
      keys = builtins.attrNames zipGroup.groups;
      len = zipGroup.length;
      indices = pkgs.lib.range 0 (len - 1);
      rows = map (
        i:
        builtins.listToAttrs (
          map (k: {
            name = k;
            value = builtins.elemAt zipGroup.groups.${k} i;
          }) keys
        )
      ) indices;
    in
    {
      members = keys;
      values = rows;
    }
  ) zipGroupEntries;

  autoParametersDependencies =
    let
      flatParameters = builtins.attrValues parameterAxes;

      extractDeps =
        val:
        if pkgs.lib.isDerivation val then
          [ val ]
        else if builtins.isList val then
          pkgs.lib.concatMap extractDeps val
        else if builtins.isAttrs val then
          pkgs.lib.concatMap extractDeps (builtins.attrValues val)
        else
          [ ];

      flattenStrings =
        val:
        if builtins.isString val then
          [ val ]
        else if builtins.isList val then
          pkgs.lib.concatMap flattenStrings val
        else if builtins.isAttrs val && !(pkgs.lib.isDerivation val) then
          pkgs.lib.concatMap flattenStrings (builtins.attrValues val)
        else
          [ ];

      nixStoreStrings = builtins.filter builtins.hasContext (
        pkgs.lib.concatMap flattenStrings flatParameters
      );

      paramPathsClosure =
        if nixStoreStrings == [ ] then
          [ ]
        else
          [
            (pkgs.runCommand "param-store-paths" { } ''
              mkdir -p $out/share/repx/${name}
              cat > $out/share/repx/${name}/param-paths.txt <<'REPX_PATHS_EOF'
              ${builtins.concatStringsSep "\n" nixStoreStrings}
              REPX_PATHS_EOF
            '')
          ];
    in
    common.uniqueDrvs (
      (pkgs.lib.flatten (map extractDeps flatParameters)) ++ paramPathsClosure ++ smartParameterContext
    );

  validateParameterAxes =
    let
      invalidParameters = pkgs.lib.filter (param: !pkgs.lib.isList param.value) (
        pkgs.lib.mapAttrsToList (name: value: { inherit name value; }) parameterAxes
      );
    in
    if invalidParameters != [ ] then
      let
        paramNames = pkgs.lib.map (p: p.name) invalidParameters;
        formattedNames = pkgs.lib.concatStringsSep ", " (map (n: ''"${n}"'') paramNames);
      in
      throw ''
        Type error in 'mkRun' parameters for run "${name}".
        All parameter values must be lists. Non-list: ${formattedNames}.
      ''
    else
      let
        emptyAxes = pkgs.lib.filter (param: param.value == [ ]) (
          pkgs.lib.mapAttrsToList (name: value: { inherit name value; }) parameterAxes
        );
      in
      if emptyAxes != [ ] then
        throw ''
          Error in 'mkRun' for run "${name}":
          Empty parameter lists: ${builtins.toJSON (map (p: p.name) emptyAxes)}.
          Empty lists produce zero combinations.
        ''
      else
        true;

  representativeParameters =
    let
      fromAxes = pkgs.lib.mapAttrs (
        _: values: if builtins.isList values && values != [ ] then builtins.head values else values
      ) parameterAxes;
      fromZips = pkgs.lib.foldl' (
        acc: zg: if zg.values != [ ] then acc // (builtins.head zg.values) else acc
      ) { } zipGroupsList;
    in
    fromAxes // fromZips;

  repxForDiscovery = repx-lib.mkPipelineHelpers {
    inherit
      pkgs
      repx-lib
      interRunDepTypes
      hashMode
      ;
    resolvedParameters = representativeParameters;
  };

  loadedPipelines = pkgs.lib.map (
    p:
    let
      pFn = if builtins.isFunction p then p else import p;
      pArgs = builtins.functionArgs pFn;
    in
    pkgs.callPackage pFn (
      if pArgs ? "repx" || pArgs ? "..." then
        {
          repx = repxForDiscovery;
        }
      else
        { }
    )
  ) pipelines;

  getDrvsFromPipeline =
    pipeline:
    let
      jobs = pkgs.lib.filter common.isVirtualJob (pkgs.lib.attrValues pipeline);
      scriptDrvs = pkgs.lib.concatMap (
        job:
        if job.repxStageType == "scatter-gather" then
          [
            job.scatterDrv
            job.gatherDrv
          ]
          ++ (builtins.attrValues job.stepDrvs)
        else
          [ job.scriptDrv ]
      ) jobs;
    in
    scriptDrvs;

  allStageDeclaredParams =
    let
      allJobs = pkgs.lib.concatMap (
        pipeline: pkgs.lib.filter common.isVirtualJob (pkgs.lib.attrValues pipeline)
      ) loadedPipelines;
    in
    pkgs.lib.unique (pkgs.lib.concatMap (job: job.declaredParameterNames or [ ]) allJobs);

  allProvidedParamNames =
    let
      fromAxes = builtins.attrNames parameterAxes;
      fromZips = pkgs.lib.concatMap (zg: zg.members) zipGroupsList;
    in
    fromAxes ++ fromZips;

  missingStageParams = pkgs.lib.subtractLists allProvidedParamNames allStageDeclaredParams;

  validateStageParams =
    if missingStageParams != [ ] then
      throw ''
        Error in 'mkRun' for run "${name}".
        Stage(s) declare parameter(s) not provided by the run: ${builtins.toJSON missingStageParams}.
        Every parameter declared by a stage must be provided by the run definition.
        Use [ null ] for parameters that are not needed in this run.
      ''
    else
      true;

  pipelineTemplates =
    assert validateStageParams;
    pkgs.lib.imap0 (
      i: pipeline:
      let
        jobs = pkgs.lib.filter common.isVirtualJob (pkgs.lib.attrValues pipeline);
      in
      {
        source =
          let
            p = builtins.elemAt pipelines i;
          in
          if builtins.isPath p then
            toString p
          else if builtins.isFunction p then
            "pipeline-fn-${toString i}"
          else
            "pipeline-${toString i}";
        stages = map (job: job.templateData) jobs;
      }
    ) loadedPipelines;

in
if invalidKeys != [ ] then
  throw ''
    Error in 'mkRun' definition for run "${name}".
    Unknown attributes: ${builtins.toJSON invalidKeys}.
    Valid: ${builtins.toJSON validKeys}.
  ''
else if !(builtins.elem hashMode validHashModes) then
  throw ''
    Error in 'mkRun' for run "${name}".
    Invalid hashMode: "${hashMode}". Valid: ${builtins.toJSON validHashModes}.
  ''
else
  assert zipCollisionAsserts;
  assert validateParameterAxes;
  let
    paramDepsClosure = pkgs.writeTextDir "share/repx/${name}/param-dependencies" (
      builtins.toJSON (parametersDependencies ++ autoParametersDependencies)
    );

    pipelineScriptDrvs = pkgs.lib.flatten (map getDrvsFromPipeline loadedPipelines);

    runImageContents =
      pipelineScriptDrvs
      ++ (common.mkRuntimePackages pkgs)
      ++ autoParametersDependencies
      ++ [ paramDepsClosure ];
  in
  {
    inherit name interRunDepTypes;

    imageContents = runImageContents;

    scriptDrvs = common.uniqueDrvs pipelineScriptDrvs;

    runTemplate = {
      inherit name;
      hash_mode = if hashMode == "pure" then "pure" else "params-only";
      inter_run_dep_types = interRunDepTypes;
      parameter_axes = parameterAxes;
      zip_groups = zipGroupsList;
      pipelines = pipelineTemplates;
      image_contents = map (d: builtins.unsafeDiscardStringContext (toString d)) runImageContents;
    };
  }
