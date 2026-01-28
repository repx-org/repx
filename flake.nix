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
      overlays.default = final: _prev: {
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
        overlays = [
          self.overlays.default
        ];
        pkgs = import nixpkgs { inherit system overlays; };
      in
      {
        packages = {
          default = pkgs.repx-runner;
          inherit (pkgs) repx-runner;
          inherit (pkgs) repx-tui;
        };

        checks = import ./nix/checks.nix {
          inherit pkgs;
          repxRunner = pkgs.repx-runner;
          referenceLab = repx-nix.packages.${system}.reference-lab;
        };

        formatter = import ./nix/formatters.nix { inherit pkgs; };

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
