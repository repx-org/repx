{ repx }:
repx.mkPipe {
  plotter = repx.callStage ./stage-plotter.nix [ ];
}
