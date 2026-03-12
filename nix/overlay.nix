final: _prev: {
  repx = final.python3Packages.toPythonModule (
    final.symlinkJoin {
      name = "repx";
      paths = [
        (final.callPackage ./pkgs/repx-rs.nix { })
        (final.callPackage ./pkgs/repx-py.nix { })
      ];
      meta = {
        mainProgram = "repx";
        description = "Reproducible HPC Experiments Framework";
      };
    }
  );
}
