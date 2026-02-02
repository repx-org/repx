{
  pkgs,
  labs,
  repx,
}:

let
  doc-assets = pkgs.callPackage ./assets.nix {
    inherit repx;
    inherit (labs) simple-lab sweep-lab;
  };

  docBaseDir = "";
in
{
  inherit doc-assets;

  logo = pkgs.runCommand "logo" { } ''
    mkdir -p $out
    cp ${../../docs/static/img/logo.svg} $out/logo.svg
  '';

  docs = pkgs.buildNpmPackage {
    name = "repx-docs";
    src = ../../docs;
    npmDepsHash = "sha256-j0DNJodnjUNlIJs6I9gjnzv1XFHM3vLIOTjsfpwuoLE=";

    preBuild = ''
      mkdir -p static/images
      find ${doc-assets} -type f -exec cp {} static/images/ \;
    '';

    buildPhase = ''
      runHook preBuild

      npm run build
      runHook postBuild
    '';
    installPhase = ''
      mkdir -p $out/${docBaseDir}
      mv build/* $out/${docBaseDir}/
    '';
  };
}
