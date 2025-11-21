{
  description = "A simple RepX example using repx-py for analysis";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    repx-nix.url = "github:repx-org/repx-nix";
    repx-py.url = "github:repx-org/repx-py";
  };

  outputs =
    {
      self,
      nixpkgs,
      repx-nix,
      repx-py,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          repx-py.overlays.default
        ];
      };
      repx-lib = repx-nix.lib;

      labOutputs = (import ./nix/lab.nix) {
        inherit pkgs repx-lib;
        gitHash = self.rev or self.dirtyRev or "unknown";
      };
    in
    {
      packages.${system} = {
        lab = labOutputs.lab;
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
