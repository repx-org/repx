{ pkgs }:

pkgs.writeShellScriptBin "clippy-fix-project" ''
  export PATH="${pkgs.clippy}/bin:${pkgs.rustfmt}/bin:${pkgs.findutils}/bin:$PATH"


  find . -type f -name "Cargo.toml" -not -path "*/target/*" -not -path "*/.git/*" | while read -r manifest; do
    crate_dir=$(dirname "$manifest")

    echo "[Clippy] Processing crate in: $crate_dir"

    pushd "$crate_dir" > /dev/null

    echo "  - Running cargo clippy --fix..."
    cargo clippy --fix --allow-dirty --allow-staged -- -D warnings || true

    echo "  - Running cargo fmt..."
    cargo fmt

    popd > /dev/null
  done
''
