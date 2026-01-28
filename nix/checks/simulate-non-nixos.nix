{ pkgs, repxRunner }:

pkgs.runCommand "check-foreign-distro-compat"
  {
    nativeBuildInputs = [ pkgs.bubblewrap ];
  }
  ''
    echo "Simulating non-NixOS environment (no /nix/store except the binary itself)..."

    BINARY="${repxRunner}/bin/repx-runner"
    REAL_BINARY=$(readlink -f "$BINARY")

    echo "Resolved binary path: $REAL_BINARY"

    if bwrap --unshare-all \
             --ro-bind "$REAL_BINARY" /repx-runner \
             --dev /dev \
             --tmpfs /tmp \
             /repx-runner --version > output.txt 2>&1; then

      echo "PASS: Binary ran successfully in isolation"
      cat output.txt
    else
      echo "FAIL: Binary failed to run in isolation. It likely has hidden dependencies."
      cat output.txt
      exit 1
    fi

    mkdir $out
  ''
