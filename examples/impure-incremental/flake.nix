{
  description = "Impure Incremental Compilation Example using RepX";

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
      repx-lib = repx.lib;

      overlaySet = import ./nix/overlay.nix;

      pkgsPure = import nixpkgs {
        inherit system;
        overlays = [
          overlaySet.common
          overlaySet.pure
        ];
      };

      pkgsImpure = import nixpkgs {
        inherit system;
        overlays = [
          overlaySet.common
          overlaySet.impure
        ];
      };

      labPure = (import ./nix/lab.nix) {
        pkgs = pkgsPure;
        inherit repx-lib;
        gitHash = self.rev or self.dirtyRev or "unknown";
      };

      labImpure = (import ./nix/lab.nix) {
        pkgs = pkgsImpure;
        inherit repx-lib;
        gitHash = self.rev or self.dirtyRev or "unknown";
      };
    in
    {
      packages.${system} = {
        default = labPure.lab;
        inherit (labPure) lab;
        "lab-impure" = labImpure.lab;

        inherit (pkgsImpure) run-impure-lab;
      };

      apps.${system}.default = {
        type = "app";
        program = "${pkgsImpure.run-impure-lab}/bin/run-impure-lab";
      };
    };
}
