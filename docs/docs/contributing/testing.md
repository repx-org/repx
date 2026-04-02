# Testing

RepX testing is fully integrated into the Nix flake. All tests run in isolated NixOS virtual machines to ensure reproducibility and proper isolation.

## Running Tests

### Full Test Suite

Run all checks:

```bash
nix flake check
```

### Individual Checks

Build and run a specific check:

```bash
nix build .#checks.x86_64-linux.<check-name>
```

List all available checks:

```bash
nix flake show --json | jq -r '.checks."x86_64-linux" | keys[]'
```

## Check Categories

### Lint Checks

Static analysis and code quality checks prefixed with `lint-`. Covers Rust (clippy, machete), Nix (deadnix, statix), shell scripts (shellcheck, shebang), and formatting.

```bash
nix build .#checks.x86_64-linux.lint-clippy
```

### Rust Unit Tests

Rust tests run inside NixOS VMs with the reference Lab available. Checks are prefixed with `rs-` and cover unit tests, integration tests, executor, client, bwrap, GC, and container runtimes.

```bash
nix build .#checks.x86_64-linux.rs-unit
nix build .#checks.x86_64-linux.rs-executor
```

### Python Tests

```bash
nix build .#checks.x86_64-linux.py-tests
```

### Nix Library Tests

Validation of `repx-lib` Nix functions. Checks are prefixed with `lib-` and cover parameter handling, pipeline DAG construction, cache invalidation, dependency resolution, and more.

### End-to-End Runtime Tests

Full execution tests in NixOS VMs. These follow naming conventions that encode the test dimensions:

- **`e2e-local-{runtime}-{mode}`** -- Local execution with a given runtime (`bwrap`, `docker`, `podman`) and sandbox mode (`pure`, `impure`, `mount-paths`).
- **`e2e-remote-{runtime}-{mode}`** -- Remote SSH execution to a NixOS target. Same runtime/mode matrix as local.
- **`non-nixos-remote-bwrap-{mode}`** -- Remote SSH execution to a simulated non-NixOS target (no `/nix` on the remote), bwrap runtime only.
- **`non-nixos-local-bwrap-impure`** -- Local execution on a simulated non-NixOS environment (no `/nix`).
- **`e2e-remote-slurm`** -- SLURM scheduler over SSH.
- Additional tests for GC, incremental sync, overlay fallback, scatter-gather, static analysis, and `node_local_path`.

```bash
nix build .#checks.x86_64-linux.e2e-local-bwrap-pure
nix build .#checks.x86_64-linux.e2e-remote-docker-impure
```

### Reference Labs

Tests use pre-built Labs as input. Each Lab targets a different testing scenario:

- **`reference-lab`** -- Standard containerized lab with a multi-stage pipeline and parameter sweeps. Used by pure, impure, and most other tests.
- **`reference-lab-native`** -- Same pipeline with `containerMode = "none"`. Used by native execution tests.
- **`reference-lab-mount-paths`** -- Parameterized lab with a job that reads from a bind-mounted host path. Used by mount-paths tests to verify the sandbox hole-punch actually works. Takes `mountDir` and `mountFile` as Nix arguments, so downstream consumers can reuse it to test their own impure paths.

```bash
nix build .#reference-lab
nix build .#reference-lab-mount-paths
```

## Development Testing

For rapid iteration during development, use the dev shell:

```bash
nix develop
cargo test --workspace
cargo test -p repx-executor
```

Note: Some tests require `REFERENCE_LAB_PATH` to be set. The Nix checks handle this automatically.

## Test Infrastructure

### repx-test-utils Crate

Shared test harness for Rust integration tests:

```rust
use repx_test_utils::harness::TestContext;

#[test]
fn test_execution() {
    let ctx = TestContext::new();
    // Isolated environment with:
    // - Temporary XDG directories
    // - Pre-staged reference Lab
    // - Configured repx settings
}
```

### Test Helpers (Nix)

Runtime tests are built from shared helpers in `nix/checks/runtime/helpers/`:

- **`mk-runtime-test.nix`** -- Generates a local e2e test for a given runtime and sandbox mode.
- **`mk-non-nixos-remote-test.nix`** -- Generates a two-VM (client + target) test simulating a non-NixOS remote via bwrap + ForceCommand.
- **`get-subset-jobs/`** -- Python package that selects a small representative subset of jobs from a Lab, avoiding running the full parameter sweep (hundreds of jobs) during CI.

### Adding a New Check

1. Create check definition in `nix/checks/`
2. Import in `nix/checks.nix`
3. The check will be available as `checks.<system>.<name>`
