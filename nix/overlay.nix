final: _prev:
let
  repx-rs = final.callPackage ./pkgs/repx-rs.nix { };
  repx-py = final.callPackage ./pkgs/repx-py.nix { };
in
{
  repx = final.python3Packages.toPythonModule (
    final.symlinkJoin {
      name = "repx-${repx-rs.version}";
      paths = [ repx-rs repx-py ];
      passthru = {
        inherit (repx-rs) version;
      };
      meta = {
        mainProgram = "repx";
        description = "Reproducible HPC Experiments Framework";
      };
    }
  );
}
