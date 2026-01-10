{
  inputs = {
    nixpkgs.follows = "repx-nix/nixpkgs";
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

        apps = {
          debug-runner = flake-utils.lib.mkApp {
            drv = repx-py;
            name = "debug-runner";
          };
          trace-params = flake-utils.lib.mkApp {
            drv = repx-py;
            name = "trace-params";
          };
          repx-viz = flake-utils.lib.mkApp {
            drv = repx-py;
            name = "repx-viz";
          };
        };

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
