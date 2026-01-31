{
  description = "RepX Monorepo: Reproducible HPC Experiment Framework";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    let
      repx-lib = import ./nix/lib/main.nix;
    in
    {
      lib = repx-lib;

      overlays.default = final: _prev: {
        repx-py = final.callPackage ./python/default.nix { };

        repx-workspace = final.pkgsStatic.callPackage ./default.nix { };

        repx =
          final.runCommand "repx"
            {
              meta.mainProgram = "repx";
            }
            ''
              mkdir -p $out/bin
              ln -s ${final.repx-workspace}/bin/repx $out/bin/repx
            '';
      };
    }
    // flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ self.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        reference-lab =
          (pkgs.callPackage ./nix/reference-lab/lab.nix {
            inherit pkgs repx-lib;
            gitHash = self.rev or self.dirtyRev or "unknown";
          }).lab;

      in
      {
        packages = {
          default = pkgs.repx;
          inherit (pkgs) repx repx-py;
          inherit reference-lab;
        };

        apps = {
          default = flake-utils.lib.mkApp {
            drv = pkgs.repx;
            name = "repx";
          };
          check-examples = flake-utils.lib.mkApp {
            drv = pkgs.callPackage ./nix/checks/check-examples.nix {
              inherit (pkgs) repx;
            };
          };
        };

        checks = import ./nix/checks.nix {
          inherit pkgs repx-lib;
          inherit (pkgs) repx;
          referenceLab = reference-lab;
        };

        formatter = import ./nix/formatters.nix { inherit pkgs; };

        devShells.default = pkgs.mkShell {
          EXAMPLE_REPX_LAB = reference-lab;
          REFERENCE_LAB_PATH = reference-lab;
          buildInputs = with pkgs; [
            openssl
            pkg-config
            rustc
            cargo
            clippy

            repx-py
            (python3.withPackages (ps: [
              ps.pytest
              ps.pandas
              ps.matplotlib
            ]))
          ];
        };
      }
    );
}
