{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  lab_version = "1.0.0";
  runs = {
    main = repx-lib.callRun ./run.nix [ ];
  };
}
