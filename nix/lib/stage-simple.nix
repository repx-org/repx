{ pkgs }:
stageDef:
let
  mkStageScript = import ./internal/mk-stage-script.nix { inherit pkgs; };

  resolvedParameters = stageDef.resolvedParameters or { };
  runDependencies = stageDef.runDependencies or [ ];

  pname = stageDef.pname or (throw "Stage must have a pname");
  version = stageDef.version or "1.1";
  inputsDef = stageDef.inputs or { };
  outputsDef = stageDef.outputs or { };

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

in
{
  _repx_virtual_job = true;
  inherit pname version scriptDrv;
  repxStageType = "simple";
  declaredParameterNames = builtins.attrNames (stageDef.parameters or { });
  outputMetadata = outputsDef;
  stageInputs = stageDef.stageInputs or { };
  resources = stageDef.resources or null;
  inputMappings = stageDef.inputMappings or [ ];

  templateData = {
    inherit pname version;
    stage_type = "simple";
    script_drv = builtins.unsafeDiscardStringContext (toString scriptDrv);
    outputs = outputsDef;
    input_mappings = stageDef.inputMappings or [ ];
    resources = stageDef.resources or null;
    parameter_defaults = stageDef.parameters or { };
    executables = {
      main = {
        inputs = stageDef.inputMappings or [ ];
        outputs = outputsDef;
      };
    };
  };
}
