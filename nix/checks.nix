{
  pkgs,
  repx,
  referenceLab,
  referenceLabNative,
  referenceLabMountPaths,
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
    e2e-local-bwrap-pure = import ./checks/runtime/e2e-local-bwrap-pure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-local-bwrap-impure = import ./checks/runtime/e2e-local-bwrap-impure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-local-bwrap-mount-paths = import ./checks/runtime/e2e-local-bwrap-mount-paths.nix {
      inherit pkgs repx;
      referenceLab = referenceLabMountPaths;
    };
    e2e-local-docker-pure = import ./checks/runtime/e2e-local-docker-pure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-local-docker-impure = import ./checks/runtime/e2e-local-docker-impure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-local-docker-mount-paths = import ./checks/runtime/e2e-local-docker-mount-paths.nix {
      inherit pkgs repx;
      referenceLab = referenceLabMountPaths;
    };
    e2e-local-podman-pure = import ./checks/runtime/e2e-local-podman-pure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-local-podman-impure = import ./checks/runtime/e2e-local-podman-impure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-local-podman-mount-paths = import ./checks/runtime/e2e-local-podman-mount-paths.nix {
      inherit pkgs repx;
      referenceLab = referenceLabMountPaths;
    };
    e2e-local-bwrap-node-local-path = import ./checks/runtime/e2e-local-bwrap-node-local-path.nix {
      inherit pkgs repx referenceLab;
    };

    e2e-remote-bwrap-pure = import ./checks/runtime/e2e-remote-bwrap-pure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-bwrap-impure = import ./checks/runtime/e2e-remote-bwrap-impure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-bwrap-mount-paths = import ./checks/runtime/e2e-remote-bwrap-mount-paths.nix {
      inherit pkgs repx;
      referenceLab = referenceLabMountPaths;
    };
    e2e-remote-docker-pure = import ./checks/runtime/e2e-remote-docker-pure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-docker-impure = import ./checks/runtime/e2e-remote-docker-impure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-docker-mount-paths = import ./checks/runtime/e2e-remote-docker-mount-paths.nix {
      inherit pkgs repx;
      referenceLab = referenceLabMountPaths;
    };
    e2e-remote-podman-pure = import ./checks/runtime/e2e-remote-podman-pure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-podman-impure = import ./checks/runtime/e2e-remote-podman-impure.nix {
      inherit pkgs repx referenceLab;
    };
    e2e-remote-podman-mount-paths = import ./checks/runtime/e2e-remote-podman-mount-paths.nix {
      inherit pkgs repx;
      referenceLab = referenceLabMountPaths;
    };
    e2e-remote-slurm = import ./checks/runtime/e2e-remote-slurm.nix { inherit pkgs repx referenceLab; };

    non-nixos-local-bwrap-impure = import ./checks/runtime/non-nixos-local-bwrap-impure.nix {
      inherit pkgs repx referenceLab;
    };
    non-nixos-remote-bwrap-pure = import ./checks/runtime/non-nixos-remote-bwrap-pure.nix {
      inherit pkgs repx referenceLab;
    };
    non-nixos-remote-bwrap-impure = import ./checks/runtime/non-nixos-remote-bwrap-impure.nix {
      inherit pkgs repx referenceLab;
    };
    non-nixos-remote-bwrap-mount-paths =
      import ./checks/runtime/non-nixos-remote-bwrap-mount-paths.nix
        {
          inherit pkgs repx;
          referenceLab = referenceLabMountPaths;
        };

    static-analysis = import ./checks/runtime/static-analysis.nix { inherit pkgs repx; };
    e2e-incremental-sync = import ./checks/runtime/e2e-incremental-sync.nix {
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
    e2e-examples = import ./checks/runtime/e2e-examples.nix {
      inherit pkgs repx repx-lib;
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
    py-tests = import ./checks/unit/repx-py.nix { inherit pkgs referenceLab; };
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
