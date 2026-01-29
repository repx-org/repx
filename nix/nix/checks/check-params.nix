{ pkgs }:
let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };

  testStage = mkSimpleStage {
    pname = "test-params-behavior";
    version = "0.0.1";

    paramInputs = {
      p_null = null;
      p_empty = "";
      p_string = "foo";
      p_space = "foo bar";
    };

    outputs = {
      out = "$out/done";
    };

    run =
      { params, outputs, ... }:
      ''
        echo "Running Parameter Expansion Tests..."

        check_count() {
          echo "$#"
        }

        cnt=$(check_count ${params.p_null})
        if [[ "$cnt" != "0" ]]; then
          echo "[FAIL] Null parameter resulted in $cnt arguments (expected 0)."
          exit 1
        fi

        cnt=$(check_count ${params.p_empty})
        if [[ "$cnt" != "1" ]]; then
          echo "[FAIL] Empty string parameter resulted in $cnt arguments (expected 1)."
          exit 1
        fi

        cnt=$(check_count ${params.p_space})
        if [[ "$cnt" != "1" ]]; then
          echo "[FAIL] Spaced string parameter resulted in $cnt arguments (expected 1)."
          exit 1
        fi

        val=${params.p_space}
        if [[ "$val" != "foo bar" ]]; then
           echo "[FAIL] Spaced string parameter mismatch. Got: '$val'"
           exit 1
        fi

        touch "${outputs.out}"
        echo "[PASS] All parameter checks passed."
      '';
  };
in
pkgs.runCommand "check-params" { } ''
  mkdir -p $out
  echo "{}" > inputs.json
  ${testStage}/bin/test-params-behavior "$out" inputs.json
''
