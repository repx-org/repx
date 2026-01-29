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

      overlays.default = final: prev: {
        repx-py = final.callPackage ./python/default.nix { };

        repx-workspace = final.pkgsStatic.callPackage ./default.nix { };

        repx-runner =
          final.runCommand "repx-runner"
            {
              meta.mainProgram = "repx-runner";
            }
            ''
              mkdir -p $out/bin
              ln -s ${final.repx-workspace}/bin/repx-runner $out/bin/repx-runner
            '';
        repx-tui =
          final.runCommand "repx-tui"
            {
              buildInputs = [ final.makeWrapper ];
              propagatedBuildInputs = [ final.repx-runner ];
              meta.mainProgram = "repx-tui";
            }
            ''
              mkdir -p $out/bin
              ln -s ${final.repx-workspace}/bin/repx-tui $out/bin/repx-tui
              wrapProgram $out/bin/repx-tui \
                --prefix PATH : ${final.repx-runner}/bin
            '';
      };
    }
    // flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ self.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        reference-lab =
          (pkgs.callPackage ./nix/nix/reference-lab/lab.nix {
            inherit pkgs repx-lib;
            gitHash = self.rev or self.dirtyRev or "unknown";
          }).lab;

        repx-py-test = pkgs.repx-py.override {
          inherit reference-lab;
        };
      in
      {
        packages = {
          default = pkgs.repx-runner;
          inherit (pkgs) repx-runner repx-tui repx-py;
          inherit reference-lab;
        };

        apps = {
          debug-runner = flake-utils.lib.mkApp {
            drv = pkgs.repx-py;
            name = "debug-runner";
          };
          trace-params = flake-utils.lib.mkApp {
            drv = pkgs.repx-py;
            name = "trace-params";
          };
          repx-viz = flake-utils.lib.mkApp {
            drv = pkgs.repx-py;
            name = "repx-viz";
          };
        };

        checks =
          (import ./nix/checks.nix {
            inherit pkgs repx-lib;
            repxRunner = pkgs.repx-runner;
            referenceLab = reference-lab;
          })
          // {
            repx-py-tests = repx-py-test;
          };

        formatter = import ./nix/nix/formatters.nix { inherit pkgs; };

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
