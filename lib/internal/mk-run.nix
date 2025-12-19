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

  allParamsRaw = params // {
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
    pkgs.lib.unique ((pkgs.lib.flatten (map extractDeps flatParams)) ++ smartParamContext);

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
      pkgs.lib.cartesianProduct allParams;

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
    pkgs.callPackage p {
      repx = repxForDiscovery;
    }
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
          # We bundle parameter dependencies into a reference file.
          # This ensures they are copied to the Nix store of the container
          # (because they are in the closure of this file),
          # but they are NOT symlinked to the root filesystem (e.g. /bin, /lib).
          # This prevents file collisions at the root when multiple data inputs
          # share filenames (e.g. two inputs both having a "docs" folder).
          paramDepsClosure = pkgs.writeTextDir "share/repx/param-dependencies" (
            builtins.toJSON (paramsDependencies ++ autoParamsDependencies)
          );
        in
        pkgs.dockerTools.buildLayeredImage {
          name = name + "-image";
          tag = "latest";
          contents = (pkgs.lib.flatten (map getDrvsFromPipeline loadedPipelines)) ++ [
            pkgs.jq
            pkgs.bash
            pkgs.coreutils
            pkgs.findutils
            pkgs.gnused
            pkgs.gawk
            pkgs.gnugrep
            paramDepsClosure
          ];
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
      in
      pkgs.callPackage pipelinePath {
        repx = repxForPipeline;
      }
    ) allCombinations;
  }
