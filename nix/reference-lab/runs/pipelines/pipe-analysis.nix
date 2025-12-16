{ repx }:

repx.mkPipe rec {
  analstage = repx.callStage ./stages/stage-analysis.nix [ ];
}
