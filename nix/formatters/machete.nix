{ pkgs }:

pkgs.writeShellScriptBin "machete-fix-project" ''
  export PATH="${pkgs.cargo-machete}/bin:${pkgs.findutils}/bin:$PATH"
  failed=0
  while read -r manifest; do
    crate_dir=$(dirname "$manifest")

    echo "[Machete] Processing crate in: $crate_dir"

    pushd "$crate_dir" > /dev/null

    echo "  - Running cargo machete --fix..."
    cargo-machete --fix || failed=1

    popd > /dev/null
  done < <(find . -type f -name "Cargo.toml" -not -path "*/target/*" -not -path "*/.git/*")

  if [ $failed -ne 0 ]; then
    echo "[Machete] Some checks failed."
    exit 1
  fi
''
