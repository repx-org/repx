{
  description = "A comprehensive tutorial for repx";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    repx-nix-lib.url = "path:../../";
  };
  outputs =
    {
      self,
      nixpkgs,
      repx-nix-lib,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
      };
      repx-lib = repx-nix-lib.lib;

      simulationRun = (pkgs.callPackage ./nix/runs/run-simulation.nix { inherit repx-lib; });

    in
    {
      packages.${system} = repx-lib.mkLab {
        inherit pkgs;
        runs = [
          simulationRun
        ];
        revision = self.rev or self.dirtyRev or "unknown";
      };

      devShells.${system} = {
        default = pkgs.mkShell {
          packages = [
            (pkgs.python3.withPackages (ps: [
              ps.matplotlib
              repx-nix-lib.packages.${system}.repx-tools
            ]))
          ];
        };
        all = pkgs.mkShell {
          packages = [
            pkgs.bash
            pkgs.jq

            (pkgs.python3.withPackages (ps: [
              ps.matplotlib
            ]))
          ];
        };
      };
    };
}
