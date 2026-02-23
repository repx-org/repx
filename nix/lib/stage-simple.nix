{ pkgs }:
stageDef:
let
  mkStageScript = import ./internal/mk-stage-script.nix { inherit pkgs; };

  paramsDef = stageDef.paramInputs or { };
  dependencyDerivations = stageDef.dependencyDerivations or [ ];
  runDependencies = stageDef.runDependencies or [ ];

  resolveWithParams =
    name: value:
    if builtins.isFunction value then
      let
        argSet = builtins.functionArgs value;
      in
      if argSet == { params = false; } || argSet == { params = true; } then
        value { params = paramsDef; }
      else
        throw ''
          Stage definition error in '${stageDef.pname or "unknown"}':
          The '${name}' attribute is a function, but it must take exactly { params } as argument.
          Got function with arguments: ${builtins.toJSON (builtins.attrNames argSet)}
        ''
    else
      value;

  pname = resolveWithParams "pname" (stageDef.pname or (throw "Stage must have a pname"));
  version = stageDef.version or "1.1";
  inputsDef = resolveWithParams "inputs" (stageDef.inputs or { });
  outputsDef = resolveWithParams "outputs" (stageDef.outputs or { });

  bashInputs = pkgs.lib.mapAttrs (name: _: "\${inputs[\"${name}\"]}") inputsDef;
  bashOutputs = outputsDef;

  sanitize =
    val:
    if builtins.isPath val then
      builtins.path {
        path = val;
        name = baseNameOf val;
      }
    else
      val;

  escapeParamValue =
    value:
    if value == null then
      ""
    else if builtins.isList value then
      builtins.concatStringsSep " " (map (v: pkgs.lib.escapeShellArg (sanitize v)) value)
    else
      pkgs.lib.escapeShellArg (sanitize value);

  bashParams = pkgs.lib.mapAttrs (_: escapeParamValue) paramsDef;

  userScript = stageDef.run {
    inputs = bashInputs;
    outputs = bashOutputs;
    params = bashParams;
    inherit pkgs;
  };

  scriptDrv = mkStageScript {
    inherit
      pname
      version
      userScript
      runDependencies
      ;
    paramInputs = paramsDef;
  };

  depders = dependencyDerivations;
  dependencyPaths = map toString depders;
  dependencyManifestJson = builtins.toJSON (map builtins.unsafeDiscardStringContext dependencyPaths);
  dependencyHash = builtins.hashString "sha256" (builtins.concatStringsSep ":" dependencyPaths);
  paramsJson = builtins.toJSON paramsDef;

in
pkgs.stdenv.mkDerivation rec {
  inherit pname version;
  dontUnpack = true;

  phases = [ "installPhase" ];

  passthru = (stageDef.passthru or { }) // {
    paramInputs = paramsDef;
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
  };

  inherit paramsJson dependencyManifestJson dependencyHash;
  passAsFile = [
    "paramsJson"
    "dependencyManifestJson"
  ];

  nativeBuildInputs = [ scriptDrv ];

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin

    cp ${scriptDrv}/bin/${pname} $out/bin/${pname}
    chmod +x $out/bin/${pname}

    cp ${scriptDrv}/${pname}-params.json $out/${pname}-params.json

    cp "$dependencyManifestJsonPath" $out/nix-input-dependencies.json

    runHook postInstall
  '';
}
