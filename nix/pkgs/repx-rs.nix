{ pkgs }:

pkgs.pkgsStatic.rustPlatform.buildRustPackage {
  pname = "repx-rs";
  version = "0.4.2";

  src = pkgs.lib.cleanSourceWith {
    src = ../../.;
    filter =
      path: _type:
      let
        p = toString path;
        root = toString ../../.;
        rel = pkgs.lib.removePrefix (root + "/") p;
      in
      p == root || rel == "Cargo.toml" || rel == "Cargo.lock" || pkgs.lib.hasPrefix "crates" rel;
  };
  doCheck = false;

  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  buildInputs = with pkgs; [
    openssl
  ];

  postInstall = ''
    mkdir -p $out/share/bash-completion/completions
    $out/bin/repx completions --shell bash > $out/share/bash-completion/completions/repx

    mkdir -p $out/share/zsh/site-functions
    $out/bin/repx completions --shell zsh > $out/share/zsh/site-functions/_repx

    mkdir -p $out/share/fish/vendor_completions.d
    $out/bin/repx completions --shell fish > $out/share/fish/vendor_completions.d/repx.fish
  '';
}
