{ pkgs }:

pkgs.rustPlatform.buildRustPackage {
  pname = "repx-runner";
  version = "0.1.0";

  src = ./.;
  doCheck = false;

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    openssl
  ];
}
