{
  pkgs,
  repx,
  referenceLab,
  referenceLabNative,
  repx-lib,
}:

let
  lintChecks = {
    "lint-deadnix" = (import ./checks/lint/deadnix.nix { inherit pkgs; }).lint;
    "lint-statix" = (import ./checks/lint/statix.nix { inherit pkgs; }).lint;
    "lint-formatting" = (import ./checks/lint/formatting.nix { inherit pkgs; }).fmt;
    "lint-shebang" = (import ./checks/lint/shebangs.nix { inherit pkgs; }).check;
    "lint-shellcheck" = (import ./checks/lint/shellcheck.nix { inherit pkgs; }).lint;
    "lint-clippy" = (import ./checks/lint/clippy.nix { inherit pkgs; }).lint;
    "lint-machete" = (import ./checks/lint/machete.nix { inherit pkgs; }).lint;
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
    e2e-bwrap-overlay-fallback = import ./checks/runtime/e2e-bwrap-overlay-fallback.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-gc-local = import ./checks/runtime/e2e-gc-local.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-gc-remote = import ./checks/runtime/e2e-gc-remote.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-scatter-gather-cancel = import ./checks/runtime/e2e-scatter-gather-cancel.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-init = import ./checks/runtime/e2e-init.nix {
      inherit pkgs repx repx-lib;
      gitHash = "check";
    };
  };

  libChecks = {
    "lib-integration" = pkgs.callPackage ./checks/lib/check-integration.nix { };
    "lib-invalidation" = pkgs.callPackage ./checks/lib/check-invalidation.nix { inherit repx-lib; };
    "lib-parameters" = pkgs.callPackage ./checks/lib/check-params.nix { };
    "lib-parameters-types" = pkgs.callPackage ./checks/lib/check-params-list.nix { };
    "lib-pipeline-logic" = pkgs.callPackage ./checks/lib/check-pipeline-logic.nix { inherit repx-lib; };
    "lib-dynamic-params-validation" =
      pkgs.callPackage ./checks/lib/check-dynamic-params-validation.nix
        {
          inherit repx-lib;
        };
    "lib-large-lab" = pkgs.callPackage ./checks/lib/check-large-lab.nix { };
    "lib-buildcommand-size" = pkgs.callPackage ./checks/lib/check-buildcommand-size.nix { };
    "lib-stage-env-size" = pkgs.callPackage ./checks/lib/check-stage-env-size.nix { };
    "lib-resource-hints" = pkgs.callPackage ./checks/lib/check-resource-hints.nix {
      inherit repx-lib;
    };
    "lib-zip-params" = pkgs.callPackage ./checks/lib/check-zip-params.nix {
      inherit repx-lib;
    };
    "lib-non-scalar-params" = pkgs.callPackage ./checks/lib/check-non-scalar-params.nix {
      inherit repx-lib;
    };
  }
  // (import ./checks/lib/check-deps.nix { inherit pkgs; });

  unitChecks = {
    repx-py-tests = import ./checks/unit/repx-py.nix { inherit pkgs referenceLab; };
    rs-client-tests = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-client-tests";
      cargoTestArgs = "--test data_only_local --test smart_sync_tests";
    };
    rs-unit = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-unit";
      cargoTestArgs = "--lib --bins";
    };
    rs-bwrap = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-bwrap";
      cargoTestArgs = "--test bwrap_tests";
    };
    rs-gc = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-gc";
      cargoTestArgs = "--test gc_tests";
    };
    rs-integration = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-integration";
      cargoTestArgs = "--test e2e_tests --test component_tests --test regression_tests";
    };
    rs-containers = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-containers";
      cargoTestArgs = "--test podman_tests --test docker_tests";
    };
    rs-executor = import ./checks/unit/repx-rs.nix {
      inherit pkgs referenceLab referenceLabNative;
      testName = "repx-rs-executor";
      cargoTestArgs = "--test executor_tests --test unit_tests";
    };
  };

in
lintChecks // runtimeChecks // libChecks // unitChecks
