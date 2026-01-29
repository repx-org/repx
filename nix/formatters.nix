{ pkgs }:

let
  treefmt = import ./formatters/treefmt.nix { inherit pkgs; };
  clippyFix = import ./formatters/clippy.nix { inherit pkgs; };
in
pkgs.writeShellScriptBin "custom-formatter" ''
  failed=0
  echo "[Formatter] Running treefmt..."
  ${treefmt}/bin/treefmt --ci -v "$@" || failed=1

  echo "[Formatter] Checking for Rust fixes..."
  if [ -z "$NIX_BUILD_TOP" ]; then
    ${clippyFix}/bin/clippy-fix-project || failed=1
  else
    echo "[Formatter] Skipping clippy (sandbox detected)."
  fi

  if [ $failed -ne 0 ]; then
    echo "[Formatter] Formatting failed."
    exit 1
  fi

  echo "[Formatter] Done."
''
