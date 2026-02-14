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

      overlays.default = import ./nix/overlay.nix;
    }
    // flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ self.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        labs = import ./nix/labs.nix {
          inherit pkgs repx-lib;
          gitHash = self.rev or self.dirtyRev or "unknown";
        };

        docsOutputs = import ./nix/docs {
          inherit pkgs labs;
          inherit (pkgs) repx;
        };
      in
      {
        packages = {
          default = pkgs.repx;
          inherit (pkgs) repx repx-py;
          inherit (labs) reference-lab reference-lab-native;
          inherit (docsOutputs) docs logo;

          repx-static = pkgs.pkgsStatic.callPackage ./default.nix { };
        };

        apps = import ./nix/apps.nix {
          inherit pkgs flake-utils;
          inherit (pkgs) repx;
          inherit (docsOutputs) docs;
        };

        checks = import ./nix/checks.nix {
          inherit pkgs repx-lib;
          inherit (pkgs) repx;
          referenceLab = labs.reference-lab;
          referenceLabNative = labs.reference-lab-native;
        };

        formatter = import ./nix/formatters.nix { inherit pkgs; };

        devShells.default = pkgs.mkShell {
          REFERENCE_LAB_PATH = labs.reference-lab;
          REFERENCE_LAB_NATIVE_PATH = labs.reference-lab-native;
          buildInputs = with pkgs; [
            openssl
            pkg-config
            rustc
            cargo
            clippy
            cargo-machete

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
