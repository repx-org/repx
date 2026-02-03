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
      tomlData = builtins.fromTOML (builtins.readFile tomlPath);
      crateName =
        tomlData.package.name
          or (if builtins.hasAttr "workspace" tomlData then "workspace-root" else "unknown");
    in
    pkgs.runCommand "${crateName}-machete"
      {
        nativeBuildInputs = [
          pkgs.cargo-machete
        ];
      }
      ''
        echo "Running cargo-machete for ${crateName}..."
        cd ${crateDir}

        cargo-machete ${crateDir}
        mkdir -p $out
        touch $out/${crateName}
      ''
  ) validCrateDirs;

in
{
  lint =
    if checks == [ ] then
      pkgs.runCommand "machete-no-crates-found" { }
        "echo 'No crates with Cargo.lock found to check' && touch $out"
    else
      pkgs.symlinkJoin {
        name = "machete-all-crates";
        paths = checks;
      };
}
