{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "repx-expand";
  version = "0.1.0";

  src = ../crates/repx-expand;

  cargoLock = {
    lockFile = ../crates/repx-expand/Cargo.lock;
  };

  nativeBuildInputs = [ pkgs.pkg-config ];

  meta = with pkgs.lib; {
    description = "Scalable job expansion engine for repx labs";
    license = licenses.mit;
  };
}
