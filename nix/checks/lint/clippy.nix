{ pkgs }:
let
  inherit (pkgs) lib;

  rootSrc = ./../../..;

  allFiles = lib.filesystem.listFilesRecursive rootSrc;
  cargoTomlPaths = builtins.filter (p: baseNameOf p == "Cargo.toml") allFiles;

  validCrateDirs = builtins.filter (
    tomlPath: builtins.pathExists ((dirOf tomlPath) + "/Cargo.lock")
  ) cargoTomlPaths;

  checks = map (
    tomlPath:
    let
      crateDir = dirOf tomlPath;
      crateName = (builtins.fromTOML (builtins.readFile tomlPath)).package.name or "unknown";
    in
    pkgs.rustPlatform.buildRustPackage {
      pname = "${crateName}-clippy";
      version = "0.0.1";

      src = crateDir;

      cargoLock = {
        lockFile = crateDir + "/Cargo.lock";
      };

      nativeBuildInputs = [
        pkgs.clippy
        pkgs.pkg-config
      ];

      buildInputs = [
        pkgs.openssl
      ];

      buildPhase = ''
        echo "Running Clippy for ${crateName}..."
        cargo clippy --all-targets --all-features -- -D warnings
        mkdir -p $out
        touch $out/${crateName}
      '';

      dontInstall = true;
      doCheck = false;
    }
  ) validCrateDirs;

in
{
  lint =
    if checks == [ ] then
      pkgs.runCommand "clippy-no-crates-found" { }
        "echo 'No crates with Cargo.lock found to check' && touch $out"
    else
      pkgs.symlinkJoin {
        name = "clippy-all-crates";
        paths = checks;
      };
}
