{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    repx-reference.url = "github:repx-org/repx?dir=examples/reference";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      repx-reference,
      ...
    }:
    {
      overlays.default = final: prev: {
        repx-runner = self.packages.${prev.system}.default;
      };
    }
    // flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [
          self.overlays.default
        ];
        pkgs = import nixpkgs { inherit system overlays; };

        repx-runner = (import ./default.nix) {
          inherit pkgs;
        };
      in
      {
        packages.default = repx-runner;

        devShells.default = pkgs.mkShell {
          EXAMPLE_REPX_LAB = repx-reference.packages.${system}.lab;
          buildInputs = with pkgs; [
            openssl
            pkg-config
            rustc
            cargo
            clippy
          ];
        };
      }
    );
}
