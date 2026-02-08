{
  pkgs,
  repx,
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
    clippy = (import ./checks/lint/clippy.nix { inherit pkgs; }).lint;
    machete = (import ./checks/lint/machete.nix { inherit pkgs; }).lint;
  };

  runtimeChecks = {
    e2e-local = import ./checks/runtime/e2e-local.nix { inherit pkgs repx referenceLab; };
    e2e-remote-local = import ./checks/runtime/e2e-remote-local.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-slurm = import ./checks/runtime/e2e-remote-slurm.nix {
      inherit pkgs repx referenceLab;
    };
    static-analysis = import ./checks/runtime/static-analysis.nix { inherit pkgs repx; };
    non-nixos-standalone = import ./checks/runtime/non-nixos-standalone.nix {
      inherit pkgs repx referenceLab;
    };
    non-nixos-remote = import ./checks/runtime/non-nixos-remote.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-impure = import ./checks/runtime/e2e-impure.nix { inherit pkgs repx referenceLab; };
    e2e-mount-paths = import ./checks/runtime/e2e-mount-paths.nix { inherit pkgs repx referenceLab; };
    e2e-impure-podman = import ./checks/runtime/e2e-impure-podman.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-mount-paths-podman = import ./checks/runtime/e2e-mount-paths-podman.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-impure-docker = import ./checks/runtime/e2e-impure-docker.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-mount-paths-docker = import ./checks/runtime/e2e-mount-paths-docker.nix {
      inherit pkgs repx referenceLab;
    };
    incremental-sync = import ./checks/runtime/incremental-sync-test.nix {
      inherit pkgs repx referenceLab;
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

  unitChecks = {
    repx-py-tests = import ./checks/unit/repx-py.nix { inherit pkgs referenceLab; };
    rs-client-tests = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-client-tests";
      cargoTestArgs = "--test wave_scheduler --test data_only_local --test smart_sync_tests";
    };
    rs-unit = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-unit";
      cargoTestArgs = "--lib --bins";
    };
    rs-bwrap = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-bwrap";
      cargoTestArgs = "--test bwrap_tests";
    };
    rs-gc = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-gc";
      cargoTestArgs = "--test gc_tests";
    };
    rs-integration = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-integration";
      cargoTestArgs = "--test e2e_tests --test component_tests --test regression_tests";
    };
    rs-containers = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-containers";
      cargoTestArgs = "--test podman_tests --test docker_tests";
    };
    rs-executor = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab;
      testName = "repx-rs-executor";
      cargoTestArgs = "--test executor_tests --test unit_tests";
    };
  };

in
lintChecks // runtimeChecks // libChecks // unitChecks
