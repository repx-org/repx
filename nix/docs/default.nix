{
  pkgs,
  labs,
  repx,
  docsVersion ? "latest",
  docsBaseUrl ? null,
}:

let
  doc-assets = pkgs.callPackage ./assets.nix {
    inherit repx;
    inherit (labs) simple-lab sweep-lab;
  };

  baseUrl = if docsBaseUrl != null then docsBaseUrl else "/";
in
{
  inherit doc-assets;

  logo = pkgs.runCommand "logo" { } ''
    mkdir -p $out
    cp ${../../docs/static/img/logo.svg} $out/logo.svg
  '';

  docs = pkgs.buildNpmPackage {
    name = "repx-docs-${docsVersion}";
    src = ../../docs;
    npmDepsHash = "sha256-QXh5kwQKVcXDJ4R1gXrau+6GI5YfssvkDQ8QusxGYLo=";

    preBuild = ''
      mkdir -p static/images
      find ${doc-assets} -type f -exec cp {} static/images/ \;
    '';

    DOCS_VERSION = docsVersion;
    DOCS_BASE_URL = baseUrl;

    buildPhase = ''
      runHook preBuild

      npm run build
      runHook postBuild
    '';
    installPhase = ''
      mkdir -p $out
      mv build/* $out/
    '';
  };
}
