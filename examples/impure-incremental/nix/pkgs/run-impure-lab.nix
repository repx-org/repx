{
  writeShellApplication,
  acl,
  nix,
  coreutils,
}:

writeShellApplication {
  name = "run-impure-lab";
  runtimeInputs = [
    acl
    nix
    coreutils
  ];
  text = ''
      PROJECT_ROOT=$(pwd)
      SRC_PATH="$PROJECT_ROOT/src"
      ACL_SCRIPT="$PROJECT_ROOT/tools/nix-acl.sh"

      if [ ! -f "$ACL_SCRIPT" ]; then
          echo "Error: tools/nix-acl.sh not found. Run from example root."
          exit 1
      fi

      echo "--> Setting ACLs for: $SRC_PATH"
      "$ACL_SCRIPT" set "$SRC_PATH"

    echo "--> Running Impure Build (Lab)..."

    nix build --impure .#lab-impure --option sandbox false -L --show-trace

    echo "--> Build Complete."
      echo "Result symlink -> ./result"
  '';
}
