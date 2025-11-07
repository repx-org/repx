{ stdenv, bash }:

stdenv.mkDerivation {
  pname = "myhello";
  version = "0.1";

  # This derivation has no source files.
  # We will generate the output directly in the installPhase.
  dontUnpack = true;

  # We don't need a build step.
  dontBuild = true;

  # The installPhase is where we create the files that will be
  # part of the final package in the Nix store.
  installPhase = ''
    # The output path is available as the '$out' environment variable.
    # Standard practice is to put binaries in '$out/bin'.
    mkdir -p $out/bin

    # Create the shell script.
    # We use a 'heredoc' (<<EOF) to write a multi-line string to a file.
    cat > $out/bin/myhello <<EOF
    #!${bash}/bin/bash
    echo "Hello from a simple Nix derivation!"
    EOF

    # Make the script executable.
    chmod +x $out/bin/myhello
  '';
}
