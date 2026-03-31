{ pkgs }:
stageDef:
let
  mkStageScript = import ./internal/mk-stage-script.nix { inherit pkgs; };
  common = import ./internal/common.nix;

  hashMode = stageDef.hashMode or "pure";
  resolvedParameters = stageDef.resolvedParameters or { };
  dependencyDerivations = stageDef.dependencyDerivations or [ ];
  runDependencies = stageDef.runDependencies or [ ];

  resolveWithParameters = common.mkResolveWithParameters resolvedParameters (
    stageDef.pname or "unknown"
  );

  pname = resolveWithParameters "pname" (stageDef.pname or (throw "Stage must have a pname"));
  version = stageDef.version or "1.1";
  inputsDef = resolveWithParameters "inputs" (stageDef.inputs or { });
  outputsDef = resolveWithParameters "outputs" (stageDef.outputs or { });

  bashInputs = pkgs.lib.mapAttrs (name: _: "\${inputs[\"${name}\"]}") inputsDef;
  bashParameters = pkgs.lib.mapAttrs (name: _: "\${parameters[\"${name}\"]}") resolvedParameters;
  bashOutputs = outputsDef;

  userScript = stageDef.run {
    inputs = bashInputs;
    outputs = bashOutputs;
    parameters = bashParameters;
    inherit pkgs;
  };

  scriptDrv = mkStageScript {
    inherit
      pname
      version
      userScript
      runDependencies
      ;
  };

  upstreamJobIds = map (j: j.jobDirName) (stageDef.upstreamJobs or [ ]);

  depMeta = common.mkDependencyMeta {
    inherit upstreamJobIds resolvedParameters;
    inherit dependencyDerivations;
  };
  inherit (depMeta) dependencyManifestJson dependencyHash parametersJson;

  hashIdentity = if hashMode == "params-only" then "${pname}-${version}" else toString scriptDrv;

  jobId = common.mkJobId [
    hashIdentity
    parametersJson
    dependencyManifestJson
    dependencyHash
    (builtins.toJSON (stageDef.inputMappings or [ ]))
  ];

  jobName = "${pname}-${version}";
  jobDirName = "${jobId}-${jobName}";

in
{
  _repx_virtual_job = true;
  inherit
    jobId
    jobName
    jobDirName
    pname
    resolvedParameters
    parametersJson
    dependencyManifestJson
    scriptDrv
    ;
  repxStageType = "simple";
  executables = {
    main = {
      inputs = stageDef.inputMappings or [ ];
      outputs = outputsDef;
    };
  };
  outputMetadata = outputsDef;
  stageInputs = stageDef.stageInputs or { };
  resources = stageDef.resources or null;
}
