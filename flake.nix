{
  inputs = {
    nixpkgs.follows = "repx-nix/nixpkgs";
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
        packages.repx-runner = repx-runner;
        packages.repx-tui = repx-runner.overrideAttrs (old: {
          meta = (old.meta or { }) // {
            mainProgram = "repx-tui";
          };
        });

        checks = {
          e2e-local = import ./tests/e2e-local.nix {
            inherit pkgs;
            repxRunner = repx-runner;
            referenceLab = repx-nix.packages.${system}.reference-lab;
          };

          e2e-remote-local = import ./tests/e2e-remote-local.nix {
            inherit pkgs;
            repxRunner = repx-runner;
            referenceLab = repx-nix.packages.${system}.reference-lab;
          };

          e2e-remote-slurm = import ./tests/e2e-remote-slurm.nix {
            inherit pkgs;
            repxRunner = repx-runner;
            referenceLab = repx-nix.packages.${system}.reference-lab;
          };
        };

        devShells.default = pkgs.mkShell {
          EXAMPLE_REPX_LAB = repx-nix.packages.${system}.reference-lab;
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
