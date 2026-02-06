final: _prev: {
  repx-py = final.callPackage ../python/default.nix { };
  repx-workspace = final.pkgsStatic.callPackage ../default.nix { };

  repx =
    final.runCommand "repx"
      {
        meta.mainProgram = "repx";
        inherit (final.repx-workspace) version;
      }
      ''
        mkdir -p $out/bin
        ln -s ${final.repx-workspace}/bin/repx $out/bin/repx
      '';
}
