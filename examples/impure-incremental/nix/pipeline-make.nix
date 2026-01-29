{ repx, ... }:

repx.mkPipe {
  build = repx.callStage ./stage-make.nix [ ];
}
