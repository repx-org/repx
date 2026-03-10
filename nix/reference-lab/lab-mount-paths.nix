{
  pkgs,
  repx-lib,
  gitHash,
  mountDir ? "/tmp/host-data",
  mountFile ? "secret.txt",
}:

let
  checkFilePath = "${mountDir}/${mountFile}";
  runDef = import ./runs/run-mount-paths.nix checkFilePath;

  runs = {
    mount-paths = repx-lib.callRun runDef [ ];
  };
in
repx-lib.mkLab {
  inherit
    pkgs
    gitHash
    repx-lib
    runs
    ;
  lab_version = "1.0.0";
  groups = {
    all = with runs; [ mount-paths ];
  };
}
