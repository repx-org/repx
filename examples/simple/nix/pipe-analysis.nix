{ repx }:

repx.mkPipe {
  # This stage depends on the metadata of the simulation run
  plotter = repx.callStage ./stage-plotter.nix [ ];
}
