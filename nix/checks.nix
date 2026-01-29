{
  pkgs,
  repxRunner,
  referenceLab,
  repx-lib,
}:

let
  lintChecks = {
    deadnix = (import ./checks/lint/deadnix.nix { inherit pkgs; }).lint;
    statix = (import ./checks/lint/statix.nix { inherit pkgs; }).lint;
    formatting = (import ./checks/lint/formatting.nix { inherit pkgs; }).fmt;
    shebang = (import ./checks/lint/shebangs.nix { inherit pkgs; }).check;
    shellcheck = (import ./checks/lint/shellcheck.nix { inherit pkgs; }).lint;
  };

  runtimeChecks = {
    e2e-local = import ./checks/runtime/e2e-local.nix {
      inherit pkgs repxRunner referenceLab;
    };
    e2e-remote-local = import ./checks/runtime/e2e-remote-local.nix {
      inherit pkgs repxRunner referenceLab;
    };
    e2e-remote-slurm = import ./checks/runtime/e2e-remote-slurm.nix {
      inherit pkgs repxRunner referenceLab;
    };
    static-analysis = import ./checks/runtime/static-analysis.nix { inherit pkgs repxRunner; };
    foreign-distro-compat = import ./checks/runtime/simulate-non-nixos.nix { inherit pkgs repxRunner; };
    e2e-impure = import ./checks/runtime/e2e-impure.nix { inherit pkgs repxRunner; };
    e2e-mount-paths = import ./checks/runtime/e2e-mount-paths.nix { inherit pkgs repxRunner; };
    e2e-impure-podman = import ./checks/runtime/e2e-impure-podman.nix { inherit pkgs repxRunner; };
    e2e-mount-paths-podman = import ./checks/runtime/e2e-mount-paths-podman.nix {
      inherit pkgs repxRunner;
    };
    e2e-impure-docker = import ./checks/runtime/e2e-impure-docker.nix { inherit pkgs repxRunner; };
    e2e-mount-paths-docker = import ./checks/runtime/e2e-mount-paths-docker.nix {
      inherit pkgs repxRunner;
    };
  };

  libChecks = {
    integration = pkgs.callPackage ./checks/lib/check-integration.nix { };
    invalidation = pkgs.callPackage ./checks/lib/check-invalidation.nix { inherit repx-lib; };
    params = pkgs.callPackage ./checks/lib/check-params.nix { };
    params_list = pkgs.callPackage ./checks/lib/check-params-list.nix { };
    pipeline_logic = pkgs.callPackage ./checks/lib/check-pipeline-logic.nix { inherit repx-lib; };
    dynamic_params_validation = pkgs.callPackage ./checks/lib/check-dynamic-params-validation.nix {
      inherit repx-lib;
    };
  }
  // (import ./checks/lib/check-deps.nix { inherit pkgs; });

in
lintChecks // runtimeChecks // libChecks
