{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    repx-nix.url = "github:repx-org/repx-nix";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      repx-nix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        repx-py = (import ./default.nix) {
          inherit pkgs;
        };
      in
      {
        packages.default = repx-py;

        overlay.default = final: prev: {
          repx-runner = self.packages.${system}.default;
        };

        devShells.default = pkgs.mkShell {
          EXAMPLE_REPX_LAB = repx-nix.packages.${system}.example-lab;
          buildInputs = with pkgs; [ python3 ];
        };
      }
    );
}
