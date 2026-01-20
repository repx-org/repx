{ pkgs }:

let
  treefmt = import ./formatters/treefmt.nix { inherit pkgs; };
  clippyFix = import ./formatters/clippy.nix { inherit pkgs; };
in
pkgs.writeShellScriptBin "custom-formatter" ''
  echo "[Formatter] Running treefmt..."
  ${treefmt}/bin/treefmt --ci -v "$@"

  echo "[Formatter] Checking for Rust fixes..."
  ${clippyFix}/bin/clippy-fix-project

  echo "[Formatter] Done."
''
