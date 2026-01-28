{ stdenv }:

let
  localDevPath =
    let
      pwd = builtins.getEnv "PWD";
      possiblePath = pwd + "/src";
    in
    if pwd == "" then
      throw ''
        -----------------------------------------------------------------------
        ERROR: Cannot determine current working directory.

        This example requires the '--impure' flag to access your local source
        files for incremental compilation.

        Please run:
          nix run --impure .
          OR
          nix build .#lab-impure --impure --option sandbox false
        -----------------------------------------------------------------------
      ''
    else if builtins.pathExists possiblePath then
      possiblePath
    else
      throw "Could not find 'src' directory at ${possiblePath}. Please run from the example root.";
in
stdenv.mkDerivation {
  name = "make-pkg";
  unpackPhase = "true";
  dontConfigure = true;
  dontFixup = true;
  __noChroot = true;

  version = if builtins ? currentTime then builtins.toString builtins.currentTime else "dirty";

  buildPhase = ''
    echo "--- IMPURE INCREMENTAL BUILD ---"
    echo "Source: ${localDevPath}"

    if [ ! -d "${localDevPath}" ]; then
        echo "ERROR: Directory ${localDevPath} does not exist!"
        exit 1
    fi

    cd "${localDevPath}"

    make > build.log 2>&1 || { cat build.log; exit 1; }
    cat build.log
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp "${localDevPath}/mybinary" $out/bin/make-pkg
  '';
}
