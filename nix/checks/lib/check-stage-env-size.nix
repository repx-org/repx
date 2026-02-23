{ pkgs, ... }:

let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };

  pad = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  mkDummyDrv = i: pkgs.writeTextDir "share/${pad}-dep-${toString i}" "${toString i}";

  depCount = 1500;
  deps = map mkDummyDrv (pkgs.lib.range 1 depCount);

  stage = mkSimpleStage {
    pname = "env-size-test-stage";
    inputs = { };
    outputs = {
      result = "$out/result.txt";
    };
    run =
      { outputs, ... }:
      ''
        echo "ok" > "${outputs.result}"
      '';
    dependencyDerivations = deps;
  };

in
stage
