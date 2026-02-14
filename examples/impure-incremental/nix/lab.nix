{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  lab_version = "1.0.0";
  runs = {
    build = repx-lib.callRun ./run-build.nix [ ];
  };
}
