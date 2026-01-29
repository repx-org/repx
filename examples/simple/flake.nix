{
  description = "A simple RepX example using repx-py for analysis";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    repx.url = "path:../../";
  };

  outputs =
    {
      self,
      nixpkgs,
      repx,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          repx.overlays.default
        ];
      };
      repx-lib = repx.lib;

      labOutputs = (import ./nix/lab.nix) {
        inherit pkgs repx-lib;
        gitHash = self.rev or self.dirtyRev or "unknown";
      };
    in
    {
      packages.${system} = {
        inherit (labOutputs) lab;
        default = labOutputs.lab;
      };

      devShells.${system} = {
        default = pkgs.mkShell {
          buildInputs = [
            pkgs.repx-py
            (pkgs.python3.withPackages (ps: [
              ps.matplotlib
              ps.pandas
            ]))
          ];
        };
      };
    };
}
