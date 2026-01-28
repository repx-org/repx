{
  pkgs,
  repxRunner,
}:

pkgs.runCommand "check-static-binary"
  {
    nativeBuildInputs = [ pkgs.file ];
  }
  ''
    echo "Checking if repx-runner is statically linked..."
    BINARY="${repxRunner}/bin/repx-runner"

    if [ ! -f "$BINARY" ]; then
      echo "Error: Binary not found at $BINARY"
      exit 1
    fi

    # Use -L to follow symlinks (since repxRunner is a wrapper/symlink farm)
    file_output=$(file -L "$BINARY")
    echo "$file_output"

    if echo "$file_output" | grep -q -E "statically linked|static-pie linked"; then
      echo "PASS: Binary is statically linked"
    else

      echo "FAIL: Binary is NOT statically linked"
      echo "Output: $file_output"
      exit 1
    fi

    mkdir $out
  ''
