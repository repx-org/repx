{ repx }:

repx.mkPipe {
  check = repx.callStage ./stages/stage-mount-path-check.nix [ ];
}
