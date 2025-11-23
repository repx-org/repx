{ repx }:
repx.mkPipe {
  generator = repx.callStage ./stage-generator.nix [ ];
}
