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
in
if invalidKeys != [ ] then
  throw ''
    Error in 'mkRun' definition for run "${name}".
    Unknown attributes were provided: ${builtins.toJSON invalidKeys}.
    The set of valid attributes is: ${builtins.toJSON validKeys}.
  ''
else
  let
    allParams = params // {
      pipeline = pipelines;
    };

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
    inherit name interRunDepTypes;

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
