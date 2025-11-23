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

        checks = {
          e2e-local = import ./tests/e2e-local.nix {
            inherit pkgs;
            repxRunner = repx-runner;
            referenceLab = repx-reference.packages.${system}.lab;
          };

          e2e-remote-local = import ./tests/e2e-remote-local.nix {
            inherit pkgs;
            repxRunner = repx-runner;
            referenceLab = repx-reference.packages.${system}.lab;
          };

          e2e-remote-slurm = import ./tests/e2e-remote-slurm.nix {
            inherit pkgs;
            repxRunner = repx-runner;
            referenceLab = repx-reference.packages.${system}.lab;
          };
        };

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
