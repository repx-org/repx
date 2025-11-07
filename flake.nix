{
  description = "A reproducible HPC experiment framework";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
        overlays = [ ];
      };
      repxLib = import ./lib/helpers.nix // {
        mkLab = import ./lib/lab.nix;
      };
      exampleLabOutputs = (import ./examples/combined/default.nix) {
        inherit pkgs;
        repx-lib = repxLib;
        revision = "1.0";
      };
    in
    {
      lib = repxLib;
      packages."x86_64-linux" = {
        example-lab = exampleLabOutputs.lab;
      };
    };
}
