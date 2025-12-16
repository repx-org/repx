{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
    repx-nix.url = "github:repx-org/repx-nix";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      repx-nix,
      ...
    }:
    (flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        reference-lab = repx-nix.packages.${system}.reference-lab;
        repx-py = (import ./default.nix) {
          inherit pkgs reference-lab;
        };
      in
      {
        packages.default = repx-py;

        devShells.default = pkgs.mkShell {
          REFERENCE_LAB_PATH = reference-lab;
          buildInputs = with pkgs; [
            (python3.withPackages (ps: [
              ps.pytest
            ]))
            repx-py
          ];
        };
      }
    ))
    // {
      overlays.default = final: _: {
        repx-py = (import ./default.nix) {
          pkgs = final;
        };
      };
    };
}
