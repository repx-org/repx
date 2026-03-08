{ pkgs }:
let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };

  testStage = mkSimpleStage {
    pname = "test-parameters-behavior";
    version = "0.0.1";

    resolvedParameters = {
      p_empty = "";
      p_string = "foo";
      p_space = "foo bar";
    };

    outputs = {
      out = "$out/done";
    };

    run =
      { parameters, outputs, ... }:
      ''
        echo "Running Parameter Expansion Tests..."

        if [[ "${parameters.p_empty}" != "" ]]; then
          echo "[FAIL] Empty parameter not empty. Got: '${parameters.p_empty}'"
          exit 1
        fi

        if [[ "${parameters.p_string}" != "foo" ]]; then
          echo "[FAIL] String parameter mismatch. Got: '${parameters.p_string}'"
          exit 1
        fi

        val="${parameters.p_space}"
        if [[ "$val" != "foo bar" ]]; then
           echo "[FAIL] Spaced string parameter mismatch. Got: '$val'"
           exit 1
        fi

        touch "${outputs.out}"
        echo "[PASS] All parameter checks passed."
      '';
  };
in
pkgs.runCommand "check-parameters" { } ''
  mkdir -p $out
  echo "{}" > inputs.json
  echo '${
    builtins.toJSON {
      p_empty = "";
      p_string = "foo";
      p_space = "foo bar";
    }
  }' > parameters.json
  ${testStage.scriptDrv}/bin/test-parameters-behavior "$out" inputs.json parameters.json
''
