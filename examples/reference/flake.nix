{
  description = "A complete repx example";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    repx-nix.url = "github:repx-org/repx-nix/main";
  };
  outputs =
    {
      self,
      nixpkgs,
      repx-nix,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          (import ./overlays.nix)
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
          packages = [
            (pkgs.python3.withPackages (ps: [
              ps.matplotlib
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
