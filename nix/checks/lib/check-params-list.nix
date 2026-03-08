{ pkgs }:
let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };

  testStage = mkSimpleStage {
    pname = "test-parameters-types";
    version = "0.0.1";

    resolvedParameters = {
      p_int = 42;
      p_string = "hello";
      p_path = "/some/path/to file.txt";
    };

    outputs = {
      out = "$out/done";
    };

    run =
      { parameters, outputs, ... }:
      ''
        echo "Running Parameter Type Tests..."

        if [[ "${parameters.p_int}" != "42" ]]; then
          echo "[FAIL] Integer parameter mismatch. Got: '${parameters.p_int}'"
          exit 1
        fi

        if [[ "${parameters.p_string}" != "hello" ]]; then
          echo "[FAIL] String parameter mismatch. Got: '${parameters.p_string}'"
          exit 1
        fi

        if [[ "${parameters.p_path}" != "/some/path/to file.txt" ]]; then
          echo "[FAIL] Path parameter mismatch. Got: '${parameters.p_path}'"
          exit 1
        fi

        touch "${outputs.out}"
        echo "[PASS] All parameter type checks passed."
      '';
  };
in
pkgs.runCommand "check-parameters-types" { } ''
  mkdir -p $out
  echo "{}" > inputs.json
  echo '${
    builtins.toJSON {
      p_int = 42;
      p_string = "hello";
      p_path = "/some/path/to file.txt";
    }
  }' > parameters.json
  ${testStage.scriptDrv}/bin/test-parameters-types "$out" inputs.json parameters.json
''
