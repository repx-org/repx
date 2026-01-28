{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  runs = {
    build = repx-lib.callRun ./run-build.nix [ ];
  };
}
