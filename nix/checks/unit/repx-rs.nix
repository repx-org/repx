{ pkgs, referenceLab }:
let
  repxSrc = (pkgs.callPackage ../../../default.nix { }).src;
  cargoDeps = pkgs.rustPlatform.importCargoLock {
    lockFile = ../../../Cargo.lock;
  };
  cargoConfig = pkgs.writeText "cargo-config.toml" ''
    [source.crates-io]
    replace-with = 'nix-sources'

    [source.nix-sources]
    directory = '${cargoDeps}'
  '';
in
pkgs.testers.runNixOSTest {
  name = "repx-rs-tests-vm";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation = {
        diskSize = 8192;
        memorySize = 4096;
        cores = 4;
      };

      environment = {
        systemPackages = with pkgs; [
          cargo
          rustc
          pkg-config
          openssl
          gcc
          git
          bubblewrap
        ];
        variables = {
          EXAMPLE_REPX_LAB = referenceLab;
          RUST_BACKTRACE = "1";
        };
      };
    };

  testScript = ''
    start_all()

    machine.succeed("cp -r ${repxSrc} /build")
    machine.succeed("chmod -R +w /build")
    machine.succeed("mkdir -p /build/.cargo")
    machine.succeed("cp ${cargoConfig} /build/.cargo/config.toml")
    machine.succeed("cd /build && git init && git config user.email 'test@test.com' && git config user.name 'Test' && git add . && git commit -m 'init'")
    machine.succeed("cd /build && cargo test --release --offline")
  '';
}
