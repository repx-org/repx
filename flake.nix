{
  description = "A reproducible HPC experiment framework";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
  };
  outputs =
    {
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

      formatter = forAllSystems (pkgs: pkgs.callPackage ./nix/formatter.nix { });
      checks = forAllSystems (pkgs: import ./nix/checks.nix { inherit pkgs repx-lib; });
    };
}
