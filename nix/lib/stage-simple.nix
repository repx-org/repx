{ pkgs }:
stageDef:
let
  mkStageScript = import ./internal/mk-stage-script.nix { inherit pkgs; };
  common = import ./internal/common.nix;

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

  depMeta = common.mkDependencyMeta {
    inherit dependencyDerivations resolvedParameters;
  };
  inherit (depMeta) dependencyManifestJson dependencyHash parametersJson;

in
pkgs.runCommand "${pname}-${version}"
  {
    inherit parametersJson dependencyManifestJson dependencyHash;
    passAsFile = [
      "parametersJson"
      "dependencyManifestJson"
    ];
    passthru = (stageDef.passthru or { }) // {
      inherit pname resolvedParameters;
      repxStageType = "simple";
      executables = {
        main = {
          inputs = stageDef.inputMappings or [ ];
          outputs = outputsDef;
        };
      };
      outputMetadata = outputsDef;
      stageInputs = stageDef.stageInputs or { };
      inherit scriptDrv;
      resources = stageDef.resources or null;
    };
  }
  ''
    mkdir -p $out/bin
    cp ${scriptDrv}/bin/${pname} $out/bin/${pname}
    chmod +x $out/bin/${pname}
    cp "$parametersJsonPath" $out/${pname}-parameters.json
    cp "$dependencyManifestJsonPath" $out/nix-input-dependencies.json
  ''
