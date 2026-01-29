{ pkgs }:
let
  analyzerScript = ../../lib/internal/analyze_deps.py;

  pyEnv = pkgs.python3.withPackages (ps: [ ps.bashlex ]);

  mkTest =
    name: shellCode: expectedFailure:
    pkgs.runCommand "test-dep-check-${name}"
      {
        nativeBuildInputs = [
          pkgs.oils-for-unix
          pyEnv
          pkgs.jq
        ];
      }
      ''
        set -euo pipefail

        cat > script.sh <<'EOF'
        ${shellCode}
        EOF

        osh -n --ast-format text script.sh > script.ast

        echo "Running analyzer..."
        set +e
        python3 ${analyzerScript} script.ast --json result.json
        EXIT_CODE=$?
        set -e

        EXPECT_FAIL="${if expectedFailure then "true" else "false"}"

        if [ "$EXPECT_FAIL" = "true" ]; then
          if [ $EXIT_CODE -eq 0 ]; then
            echo "FAIL: Expected analyzer to fail, but it succeeded."
            cat result.json || true
            exit 1
          else
            echo "SUCCESS: Analyzer failed as expected."
            if ! jq -e '.missing[] | select(. == "missing_command")' result.json > /dev/null; then
               echo "FAIL: JSON output did not contain 'missing_command'"
               cat result.json
               exit 1
            fi
          fi
        else
          if [ $EXIT_CODE -ne 0 ]; then
            echo "FAIL: Expected analyzer to succeed, but it failed."
            cat result.json || true
            exit 1
          else
            echo "SUCCESS: Analyzer succeeded."
          fi
        fi

        mkdir -p $out
      '';
in
{
  fail_missing = mkTest "fail-missing" ''
    echo "starting"
    missing_command "some args"
  '' true;

  pass_valid = mkTest "pass-valid" ''
    mkdir -p /tmp/foo
    ls -la
    echo "done"
  '' false;

  pass_complex = mkTest "pass-complex" ''
    # echo "commented_out"
    my_var="some_val"
    echo "$my_var"

    # Function definition shouldn't trigger 'my_func' as missing
    my_func() {
      true
    }
    my_func
  '' false;
}
