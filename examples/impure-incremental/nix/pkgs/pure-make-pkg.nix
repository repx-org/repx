{ stdenv }:

stdenv.mkDerivation {
  name = "make-pkg";
  src = ../../src;

  buildPhase = ''
    make
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp mybinary $out/bin/make-pkg
  '';
}
