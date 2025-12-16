{
  description = "A reproducible HPC experiment framework";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
  };
  outputs =
    {
      self,
      nixpkgs,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "i686-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      repx-lib = import ./lib/main.nix;
    in
    {
      lib = repx-lib;
      packages = forAllSystems (pkgs: {
        reference-lab =
          (pkgs.callPackage ./nix/reference-lab/lab.nix {
            inherit pkgs repx-lib;
            gitHash = self.rev or self.dirtyRev or "unknown";
          }).lab;
      });
      formatter = forAllSystems (pkgs: import ./nix/formatters.nix { inherit pkgs; });
      checks = forAllSystems (pkgs: import ./nix/checks.nix { inherit pkgs repx-lib; });
    };
}
