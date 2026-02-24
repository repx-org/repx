{
  docsVersion ? "latest",
  system ? builtins.currentSystem,
}:
let
  flakeLock = builtins.fromJSON (builtins.readFile ../../flake.lock);
  nixpkgsInfo = flakeLock.nodes.nixpkgs.locked;
  nixpkgs = builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/${nixpkgsInfo.rev}.tar.gz";
    sha256 = nixpkgsInfo.narHash;
  };

  repx-lib = import ../lib/main.nix;
  overlays = [ (import ../overlay.nix) ];
  pkgs = import nixpkgs { inherit system overlays; };

  labs = import ../labs.nix {
    inherit pkgs repx-lib;
    gitHash = "release";
  };

  baseUrl = if docsVersion == "latest" then "/latest/" else "/${docsVersion}/";

  docsOutputs = import ./default.nix {
    inherit pkgs labs docsVersion;
    docsBaseUrl = baseUrl;
    inherit (pkgs) repx;
  };
in
docsOutputs.docs
