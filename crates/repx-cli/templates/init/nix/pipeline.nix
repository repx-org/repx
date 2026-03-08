{ repx }:

repx.mkPipe {
  hello = repx.callStage ./stage-hello.nix [ ];
}
