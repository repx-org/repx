{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    repx-nix.url = "github:repx-org/repx-nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      repx-nix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        repx-runner = (import ./default.nix) {
          inherit pkgs;
        };
      in
      {
        packages.default = repx-runner;

        overlay.default = final: prev: {
          repx-runner = self.packages.${system}.default;
        };

        devShells.default = pkgs.mkShell {
          EXAMPLE_REPX_LAB = repx-nix.packages.${system}.example-lab;
          buildInputs = with pkgs; [
            openssl
            pkg-config
            rustc
            cargo
          ];
        };
      }
    );
}
