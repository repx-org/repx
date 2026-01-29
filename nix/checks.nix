{
  pkgs,
  repxRunner,
  referenceLab,
  repx-lib,
}:

let
  lintChecks = {
    deadnix = (import ./lint/deadnix.nix { inherit pkgs; }).lint;
    statix = (import ./lint/statix.nix { inherit pkgs; }).lint;
    formatting = (import ./lint/formatting.nix { inherit pkgs; }).fmt;
    shebang = (import ./lint/shebangs.nix { inherit pkgs; }).check;
    shellcheck = (import ./lint/shellcheck.nix { inherit pkgs; }).lint;
  };

  runtimeChecks = {
    e2e-local = import ./runtime/e2e-local.nix {
      inherit pkgs repxRunner referenceLab;
    };
    e2e-remote-local = import ./runtime/e2e-remote-local.nix {
      inherit pkgs repxRunner referenceLab;
    };
    e2e-remote-slurm = import ./runtime/e2e-remote-slurm.nix {
      inherit pkgs repxRunner referenceLab;
    };
    static-analysis = import ./runtime/static-analysis.nix { inherit pkgs repxRunner; };
    foreign-distro-compat = import ./runtime/simulate-non-nixos.nix { inherit pkgs repxRunner; };
    e2e-impure = import ./runtime/e2e-impure.nix { inherit pkgs repxRunner; };
    e2e-mount-paths = import ./runtime/e2e-mount-paths.nix { inherit pkgs repxRunner; };
    e2e-impure-podman = import ./runtime/e2e-impure-podman.nix { inherit pkgs repxRunner; };
    e2e-mount-paths-podman = import ./runtime/e2e-mount-paths-podman.nix { inherit pkgs repxRunner; };
    e2e-impure-docker = import ./runtime/e2e-impure-docker.nix { inherit pkgs repxRunner; };
    e2e-mount-paths-docker = import ./runtime/e2e-mount-paths-docker.nix { inherit pkgs repxRunner; };
    check-invalidation = import ./runtime/check-invalidation.nix {
      inherit pkgs repxRunner referenceLab;
    };
  };

  libChecks = {
    integration = pkgs.callPackage ./lib/check-integration.nix { };
    invalidation = pkgs.callPackage ./lib/check-invalidation.nix { inherit repx-lib; };
    params = pkgs.callPackage ./lib/check-params.nix { };
    params_list = pkgs.callPackage ./lib/check-params-list.nix { };
    pipeline_logic = pkgs.callPackage ./lib/check-pipeline-logic.nix { inherit repx-lib; };
    dynamic_params_validation = pkgs.callPackage ./lib/check-dynamic-params-validation.nix {
      inherit repx-lib;
    };
  }
  // (import ./lib/check-deps.nix { inherit pkgs; });

in
lintChecks // runtimeChecks // libChecks
