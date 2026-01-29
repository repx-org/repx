{ pkgs }:
let
  mkSimpleStage = import ../../lib/stage-simple.nix { inherit pkgs; };

  testStage = mkSimpleStage {
    pname = "test-params-list-behavior";
    version = "0.0.1";

    paramInputs = {
      p_list = [
        "-f"
        "/path/to file.txt"
      ];
      p_single = [ "single" ];
      p_empty_list = [ ];
    };

    outputs = {
      out = "$out/done";
    };

    run =
      { params, outputs, ... }:
      ''
        echo "Running List Parameter Expansion Tests..."

        check_count() {
          echo "$#"
        }

        cnt=$(check_count ${params.p_list})
        if [[ "$cnt" != "2" ]]; then
          echo "[FAIL] List parameter resulted in $cnt arguments (expected 2)."
          exit 1
        fi

        cnt=$(check_count ${params.p_single})
        if [[ "$cnt" != "1" ]]; then
          echo "[FAIL] Single element list resulted in $cnt arguments (expected 1)."
          exit 1
        fi

        cnt=$(check_count ${params.p_empty_list})
        if [[ "$cnt" != "0" ]]; then
          echo "[FAIL] Empty list resulted in $cnt arguments (expected 0)."
          exit 1
        fi

        touch "${outputs.out}"
        echo "[PASS] All list parameter checks passed."
      '';
  };
in
pkgs.runCommand "check-params-list" { } ''
  mkdir -p $out
  echo "{}" > inputs.json
  ${testStage}/bin/test-params-list-behavior "$out" inputs.json
''
