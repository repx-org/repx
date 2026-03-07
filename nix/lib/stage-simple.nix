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
pkgs.stdenv.mkDerivation rec {
  inherit pname version;
  dontUnpack = true;

  phases = [ "installPhase" ];

  passthru = (stageDef.passthru or { }) // {
    inherit resolvedParameters;
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

  inherit parametersJson dependencyManifestJson dependencyHash;
  passAsFile = [
    "parametersJson"
    "dependencyManifestJson"
  ];

  nativeBuildInputs = [ scriptDrv ];

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin

    cp ${scriptDrv}/bin/${pname} $out/bin/${pname}
    chmod +x $out/bin/${pname}

    cp "$parametersJsonPath" $out/${pname}-parameters.json

    cp "$dependencyManifestJsonPath" $out/nix-input-dependencies.json

    runHook postInstall
  '';
}
