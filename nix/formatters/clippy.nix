{ pkgs }:

pkgs.writeShellScriptBin "clippy-fix-project" ''
  export PATH="${pkgs.clippy}/bin:${pkgs.rustfmt}/bin:${pkgs.findutils}/bin:$PATH"



  failed=0
  while read -r manifest; do
    crate_dir=$(dirname "$manifest")

    echo "[Clippy] Processing crate in: $crate_dir"

    pushd "$crate_dir" > /dev/null

    echo "  - Running cargo clippy --fix..."
    ${pkgs.cargo}/bin/cargo clippy --fix --allow-dirty --allow-staged --allow-no-vcs -- -D warnings || failed=1

    echo "  - Running cargo fmt..."
    ${pkgs.cargo}/bin/cargo fmt || failed=1

    popd > /dev/null
  done < <(find . -type f -name "Cargo.toml" -not -path "*/target/*" -not -path "*/.git/*")

  if [ $failed -ne 0 ]; then
    echo "[Clippy] Some checks failed."
    exit 1
  fi
''
