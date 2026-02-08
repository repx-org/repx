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

## Check Categories

### Lint Checks

Static analysis and code quality:

| Check | Description |
|-------|-------------|
| `clippy` | Rust linter warnings |
| `formatting` | Code style validation |
| `machete` | Unused Cargo dependencies |
| `deadnix` | Dead Nix code |
| `statix` | Nix anti-patterns |
| `shellcheck` | Shell script analysis |
| `shebang` | Script interpreter validation |

### Rust Unit Tests

Rust tests run inside NixOS VMs with the reference Lab available:

| Check | Scope |
|-------|-------|
| `rs-unit` | Library and binary unit tests |
| `rs-integration` | End-to-end, component, regression tests |
| `rs-executor` | Executor crate tests |
| `rs-client-tests` | Client crate tests (wave scheduler, sync) |
| `rs-bwrap` | Bubblewrap runtime tests |
| `rs-gc` | Garbage collection tests |
| `rs-containers` | Docker and Podman tests |

Example:

```bash
nix build .#checks.x86_64-linux.rs-unit
nix build .#checks.x86_64-linux.rs-executor
```

### Python Tests

```bash
nix build .#checks.x86_64-linux.repx-py-tests
```

### Nix Library Tests

Validation of the `repx-lib` Nix functions:

| Check | Description |
|-------|-------------|
| `integration` | Full Lab build validation |
| `invalidation` | Cache invalidation behavior |
| `params` | Parameter handling |
| `params_list` | Parameter list processing |
| `pipeline_logic` | Pipeline DAG construction |
| `dynamic_params_validation` | Dynamic parameter validation |
| `pass_valid` | Dependency check (valid case) |
| `pass_complex` | Complex dependency scenarios |
| `fail_missing` | Missing dependency detection |

### End-to-End Tests

Full execution tests in NixOS VMs:

| Check | Description |
|-------|-------------|
| `e2e-local` | Local bwrap execution |
| `e2e-impure` | Impure mode with host mounts |
| `e2e-impure-docker` | Docker container execution |
| `e2e-impure-podman` | Podman container execution |
| `e2e-mount-paths` | Explicit mount path configuration |
| `e2e-mount-paths-docker` | Docker with mount paths |
| `e2e-mount-paths-podman` | Podman with mount paths |
| `e2e-remote-local` | SSH target, local scheduler |
| `e2e-remote-slurm` | SSH target, SLURM scheduler |
| `incremental-sync` | Incremental image synchronization |
| `non-nixos-standalone` | Execution on non-NixOS host |
| `non-nixos-remote` | Remote execution to non-NixOS target |
| `static-analysis` | Static binary validation |

Example:

```bash
nix build .#checks.x86_64-linux.e2e-local
nix build .#checks.x86_64-linux.incremental-sync
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

### Reference Lab

The `reference-lab` package provides a pre-built Lab for testing:

```bash
nix build .#reference-lab
```

Tests automatically use this Lab via `REFERENCE_LAB_PATH`.

## Writing Tests

### Rust Guidelines

1. Unit tests: `mod tests` blocks in source files
2. Integration tests: `tests/` directory in crate root
3. Use `TestContext` for tests requiring Lab artifacts
4. Use `#[tokio::test]` for async tests

### Adding a New Check

1. Create check definition in `nix/checks/`
2. Import in `nix/checks.nix`
3. The check will be available as `checks.<system>.<name>`
