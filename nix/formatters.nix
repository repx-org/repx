{ pkgs }:

let
  treefmt = import ./formatters/treefmt.nix { inherit pkgs; };
in
pkgs.writeShellScriptBin "custom-formatter" ''
  echo "[Formatter] Running treefmt..."
  ${treefmt}/bin/treefmt --ci -v "$@"
  echo "[Formatter] Done."
''
