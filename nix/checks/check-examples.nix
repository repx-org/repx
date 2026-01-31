{
  pkgs,
  repx,
}:

pkgs.writeShellApplication {
  name = "check-examples";

  runtimeInputs = [
    pkgs.nix
    pkgs.jq
    pkgs.gawk
    repx
  ];

  text = ''
    set -euo pipefail

    if [ ! -d "examples" ]; then
      echo "Error: 'examples' directory not found."
      echo "Please run this command from the root of the repx repository."
      exit 1
    fi

    EXAMPLES_DIR="$PWD/examples"

    echo "Running example checks..."

    examples=(
      "simple"
      "param-sweep"
      "impure-incremental"
    )

    failed=0

    for example in "''${examples[@]}"; do
      echo "----------------------------------------------------------------"
      echo "Checking example: $example"
      echo "----------------------------------------------------------------"

      example_path="$EXAMPLES_DIR/$example"

      if [ ! -d "$example_path" ]; then
        echo "Error: Example directory not found: $example_path"
        failed=1
        continue
      fi

      pushd "$example_path" > /dev/null

      echo "Building lab for $example..."
      if lab_path=$(nix build .#lab --impure --print-out-paths --no-link); then
        echo "Lab built at: $lab_path"

        echo "Running repx checks..."

        run_name=$(repx list runs --lab "$lab_path" | tail -n +2 | head -n 1 | awk '{print $1}')

        if [ -n "$run_name" ]; then
            echo "Checking jobs for run: $run_name"
            if ! repx list jobs "$run_name" --lab "$lab_path"; then
                echo "FAIL: Failed to list jobs for run $run_name in $example"
                failed=1
            else
                echo "PASS: $example passed checks"
            fi
        else
            echo "PASS: $example built and listed runs (no runs found)"
        fi

      else
        echo "FAIL: $example failed to build"
        failed=1
      fi

      popd > /dev/null
    done

    if [ "$failed" -eq 0 ]; then
      echo "All examples passed!"
      exit 0
    else
      echo "Some examples failed."
      exit 1
    fi
  '';
}
