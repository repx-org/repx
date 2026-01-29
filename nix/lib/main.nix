let
  callRun = runPath: dependencies: {
    _repx_type = "run_placeholder";
    inherit runPath dependencies;
  };

  mkRun = import ./internal/mk-run.nix;
  mkUtils = import ./utils.nix;

  mkLab = import ./lab.nix;
in
rec {
  inherit
    mkRun
    mkLab
    callRun
    mkUtils
    ;
  mkPipelineHelpers =
    args:
    let
      callStageImpl = import ./internal/call-stage.nix (
        args
        // {
          repx-lib = {
            inherit mkPipelineHelpers;
          };
        }
      );
    in
    {
      mkPipe = stages: stages;
      callStage = stageFile: dependencies: callStageImpl stageFile dependencies;
    };
}
