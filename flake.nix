{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
    repx-reference.url = "github:repx-org/repx?dir=examples/reference";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      repx-reference,
      ...
    }:
    (flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        reference-lab = repx-reference.packages.${system}.lab;
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
