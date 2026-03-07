{
  pkgs,
  repx-lib,
  name,
  containerized ? true,
  pipelines,
  params,
  paramsDependencies ? [ ],
  dependencyJobs ? { },
  interRunDepTypes ? { },
  ...
}@args:
let
  validKeys = [
    "pkgs"
    "repx-lib"
    "name"
    "containerized"
    "pipelines"
    "params"
    "paramsDependencies"
    "dependencyJobs"
    "interRunDepTypes"
    "override"
    "overrideDerivation"
  ];

  actualKeys = builtins.attrNames args;
  invalidKeys = pkgs.lib.subtractLists validKeys actualKeys;

  zipGroupEntries = pkgs.lib.filterAttrs (
    _: val: builtins.isAttrs val && (val._repx_zip or false)
  ) params;

  normalParams = pkgs.lib.filterAttrs (
    _: val: !(builtins.isAttrs val && (val._repx_zip or false))
  ) params;

  normalParamNames = builtins.attrNames normalParams;

  allZipMembers = pkgs.lib.concatLists (
    pkgs.lib.mapAttrsToList (
      anchor: zipGroup:
      map (member: {
        inherit anchor member;
      }) (builtins.attrNames zipGroup.groups)
    ) zipGroupEntries
  );

  zipVsNormalCollisions = builtins.filter (m: builtins.elem m.member normalParamNames) allZipMembers;

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
        A parameter name can only appear once -- either as a normal parameter or inside a zip group, not both.
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
        A parameter name can only appear in one zip group.
      ''
    else if anchorVsMemberCollisions != [ ] then
      let
        first = builtins.head anchorVsMemberCollisions;
      in
      throw ''
        Parameter collision in run "${name}".
        '${first.anchor}' is used as both the zip anchor key and a member name inside it.
        Use a different anchor key name for the utils.zip group.
      ''
    else
      true;

  zipGroupToList =
    zipGroup:
    let
      keys = builtins.attrNames zipGroup.groups;
      len = zipGroup.length;
      indices = pkgs.lib.range 0 (len - 1);
    in
    map (
      i:
      builtins.listToAttrs (
        map (k: {
          name = k;
          value = builtins.elemAt zipGroup.groups.${k} i;
        }) keys
      )
    ) indices;

  zipDimensions = pkgs.lib.imap0 (
    i: _name:
    let
      zipGroup = zipGroupEntries.${_name};
      zippedList = zipGroupToList zipGroup;
    in
    {
      name = "_repx_zip_${toString i}";
      value = zippedList;
    }
  ) (builtins.attrNames zipGroupEntries);

  zipDimensionsAttrs = builtins.listToAttrs zipDimensions;
  zipSyntheticKeys = map (d: d.name) zipDimensions;

  allParamsRaw =
    assert zipCollisionAsserts;
    normalParams
    // zipDimensionsAttrs
    // {
      pipeline = pipelines;
    };

  processedParams = pkgs.lib.mapAttrs (
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
  ) allParamsRaw;

  allParams = pkgs.lib.mapAttrs (_: p: p.values) processedParams;
  smartParamContext = pkgs.lib.flatten (pkgs.lib.mapAttrsToList (_: p: p.context) processedParams);

  common = import ./common.nix;

  autoParamsDependencies =
    let
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

      flatParams = builtins.attrValues allParams;
    in
    common.uniqueDrvs ((pkgs.lib.flatten (map extractDeps flatParams)) ++ smartParamContext);

  allCombinations =
    let
      invalidParams = pkgs.lib.filter (param: !pkgs.lib.isList param.value) (
        pkgs.lib.mapAttrsToList (name: value: { inherit name value; }) allParams
      );
    in
    if invalidParams != [ ] then
      let
        paramNames = pkgs.lib.map (p: p.name) invalidParams;
        formattedNames = pkgs.lib.concatStringsSep ", " (map (n: ''"${n}"'') paramNames);
      in
      throw ''
        Type error in 'mkRun' parameters for run "${name}".
        The 'cartesianProduct' function for parameter sweeps expects all parameter values to be lists.
        The following parameters have non-list values: ${formattedNames}.

        Please ensure each parameter value is wrapped in a list, e.g., 'param = [ "value" ];'
      ''
    else
      let
        nonZipParams = pkgs.lib.filterAttrs (n: _: !(builtins.elem n zipSyntheticKeys)) allParams;
        paramsWithNulls = pkgs.lib.filter (param: builtins.any (elem: elem == null) param.value) (
          pkgs.lib.mapAttrsToList (name: value: { inherit name value; }) nonZipParams
        );
      in
      if paramsWithNulls != [ ] then
        let
          nullParamNames = pkgs.lib.map (p: p.name) paramsWithNulls;
          formattedNullNames = pkgs.lib.concatStringsSep ", " (map (n: ''"${n}"'') nullParamNames);
        in
        throw ''
          Type error in 'mkRun' parameters for run "${name}".
          The following parameter lists contain null values: ${formattedNullNames}.
          Null values in parameter lists are not allowed. Please remove them or replace with valid values.
        ''
      else
        let
          rawCombinations = pkgs.lib.cartesianProduct allParams;
        in
        map (
          combo:
          let
            zipAttrs = pkgs.lib.foldl' (acc: key: acc // (combo.${key} or { })) { } zipSyntheticKeys;
          in
          (pkgs.lib.removeAttrs combo zipSyntheticKeys) // zipAttrs
        ) rawCombinations;

  repxForDiscovery = repx-lib.mkPipelineHelpers {
    inherit pkgs repx-lib interRunDepTypes;
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
in
if invalidKeys != [ ] then
  throw ''
    Error in 'mkRun' definition for run "${name}".
    Unknown attributes were provided: ${builtins.toJSON invalidKeys}.
    The set of valid attributes is: ${builtins.toJSON validKeys}.
  ''
else if allCombinations == [ ] then
  throw ''
    Error in 'mkRun' for run "${name}":
    The resulting parameter sweep is empty.
    This happens if the 'pipelines' list is empty, or if any parameter in 'params' is an empty list.
    'pkgs.lib.cartesianProduct' produces no combinations if *any* input list is empty.
  ''
else
  {
    inherit name interRunDepTypes;

    image =
      if containerized then
        let
          paramDepsClosure = pkgs.writeTextDir "share/repx/param-dependencies" (
            builtins.toJSON (paramsDependencies ++ autoParamsDependencies)
          );
        in
        pkgs.dockerTools.buildLayeredImage {
          name = name + "-image";
          tag = "latest";
          compressor = "none";
          contents =
            (pkgs.lib.flatten (map getDrvsFromPipeline loadedPipelines))
            ++ (common.mkRuntimePackages pkgs)
            ++ [ paramDepsClosure ];
          config = {
            Cmd = [ "${pkgs.bash}/bin/bash" ];
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
          inherit
            pkgs
            repx-lib
            paramInputs
            dependencyJobs
            interRunDepTypes
            ;
        };

        pipelineFn = if builtins.isFunction pipelinePath then pipelinePath else import pipelinePath;
        pipelineArgs = builtins.functionArgs pipelineFn;
      in
      pkgs.callPackage pipelineFn (
        if pipelineArgs ? "repx" || pipelineArgs ? "..." then
          {
            repx = repxForPipeline;
          }
        else
          { }
      )
    ) allCombinations;
  }
