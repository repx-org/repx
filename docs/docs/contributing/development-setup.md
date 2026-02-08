# Development Setup

RepX development is fully Nix-based. All build, test, and documentation workflows are defined as flake outputs.

## Prerequisites

| Requirement | Version | Notes |
|-------------|---------|-------|
| Nix | 2.4+ | Flakes and `nix-command` enabled |

Enable experimental features in `~/.config/nix/nix.conf`:

```
experimental-features = nix-command flakes
```

## Flake Structure

Inspect available outputs:

```bash
nix flake show
```

### Packages

| Output | Description |
|--------|-------------|
| `packages.repx` | The RepX CLI binary |
| `packages.repx-static` | Statically linked binary (musl) |
| `packages.repx-py` | Python analysis library |
| `packages.docs` | Built documentation site |
| `packages.reference-lab` | Reference Lab for testing |

### Apps

| Output | Description |
|--------|-------------|
| `apps.default` | Run the RepX binary |
| `apps.docs-preview` | Build and serve documentation locally |
| `apps.check-repx-examples` | Validate example experiments |

### Development Shell

Enter the development environment:

```bash
nix develop
```

The shell provides all build dependencies without polluting the global environment.

## Building

### Build the CLI

```bash
nix build
# Output: ./result/bin/repx
```

### Build Static Binary

```bash
nix build .#repx-static
```

### Build Documentation

```bash
nix build .#docs
```

### Build Python Package

```bash
nix build .#repx-py
```

## Development Workflow

For iterative development, use the dev shell with cargo:

```bash
nix develop
cargo build --workspace
cargo test --workspace
```

### Crate Structure

| Crate | Purpose |
|-------|---------|
| `repx-cli` | Command-line interface |
| `repx-runner` | Job scheduling and submission |
| `repx-core` | Shared types, configuration, logging |
| `repx-executor` | Runtime abstraction and process execution |
| `repx-client` | Target backends (local, SSH, SLURM) |
| `repx-tui` | Terminal user interface |
| `repx-viz` | Topology visualization |
| `repx-test-utils` | Test harness and fixtures |

## Documentation

Preview documentation locally:

```bash
nix run .#docs-preview
```

This builds the documentation and serves it at `http://localhost:8080/`.

## Code Quality

All linting is available as flake checks:

```bash
# Run all checks
nix flake check

# Run specific lint check
nix build .#checks.x86_64-linux.clippy
nix build .#checks.x86_64-linux.formatting
nix build .#checks.x86_64-linux.machete
```

### Available Lint Checks

| Check | Description |
|-------|-------------|
| `clippy` | Rust linter |
| `formatting` | Code formatting validation |
| `machete` | Unused dependency detection |
| `deadnix` | Dead Nix code detection |
| `statix` | Nix static analysis |
| `shellcheck` | Shell script linting |
| `shebang` | Shebang line validation |

### Format Code

```bash
nix fmt
```

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `REFERENCE_LAB_PATH` | Path to reference Lab (set automatically in checks) |
| `REPX_LOG_LEVEL` | Log verbosity: `error`, `warn`, `info`, `debug`, `trace` |

## IDE Configuration

### rust-analyzer

The workspace is configured for `rust-analyzer`. Ensure your editor loads the workspace `Cargo.toml`.

Enter the dev shell before starting your editor to ensure tooling is available:

```bash
nix develop
code .
```

### Direnv Integration

For automatic shell activation, create `.envrc`:

```bash
use flake
```

Then run `direnv allow`.
