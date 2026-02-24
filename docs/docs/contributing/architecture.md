# Architecture

RepX is a hybrid system combining the build-time guarantees of Nix with a runtime orchestration layer written in Rust. This document describes the internal structure and design rationale of the framework.

## System Overview

The architecture follows a strict separation of concerns across three layers: definition, execution, and analysis. Each layer operates independently and communicates through well-defined interfaces.

## Components

### The Definition Layer (Nix Library)

The `repx` Nix library transforms experiment specifications into deterministic build artifacts.

| Aspect | Description |
|--------|-------------|
| Input | Nix expressions defining stages, pipelines, and parameters |
| Output | A self-contained "Lab" derivation |
| Mechanism | Dependency resolution, software fetching, metadata generation |

The Nix layer ensures reproducibility by capturing the complete dependency closure at build time. All software versions, environment variables, and build instructions are immutably recorded.

### The Execution Layer (Rust CLI)

The `repx` CLI orchestrates experiment execution across heterogeneous compute environments.

**Crate Structure:**

| Crate | Responsibility |
|-------|----------------|
| `repx-cli` | Command-line interface and argument parsing |
| `repx-runner` | DAG traversal, job scheduling, submission logic |
| `repx-core` | Shared types, configuration, logging, error handling |
| `repx-executor` | Process execution, container runtime abstraction |
| `repx-client` | Target backends (local, SSH, SLURM) |
| `repx-tui` | Terminal user interface for monitoring |
| `repx-viz` | Topology visualization |
| `repx-test-utils` | Shared test harness and fixtures |

**Executor Module Structure:**

The `repx-executor` crate provides runtime abstraction through a modular design:

```
repx-executor/
  src/
    lib.rs          # Public API and Executor implementation
    context.rs      # RuntimeContext for shared state
    error.rs        # ExecutorError types
    util.rs         # Helper functions
    runtime/
      mod.rs        # Runtime trait and enum
      native.rs     # Direct process execution
      bwrap.rs      # Bubblewrap sandboxing
      container.rs  # Docker/Podman support
```

The `Runtime` enum abstracts over execution backends:

- **Native**: Direct process spawning on the host system
- **Bwrap**: Bubblewrap-based sandboxed execution
- **Docker/Podman**: OCI container runtimes

### The Analysis Layer (Python Library)

The `repx-py` library provides programmatic access to experiment results.

| Aspect | Description |
|--------|-------------|
| Input | Lab manifest and output store paths |
| Output | Queryable Python objects for results and metadata |
| Use Case | Jupyter notebooks, post-processing scripts, visualization |

## The Lab Artifact

The Lab directory structure serves as the interface contract between the definition and execution layers:

```
<lab-root>/
  lab/
    lab-metadata.json    # Lab ID and root metadata reference
  revision/
    metadata.json        # DAG structure, run definitions
    <run-hash>.json      # Per-run job manifests
  jobs/
    <job-hash>/
      bin/               # Executable scripts
      out/               # Expected output structure
  host-tools/
    <hash>/
      bin/               # Static binaries (rsync, bash, jq, etc.)
  image/                 # Container images (when applicable)
    <image-hash>/
```

This structure enables the CLI to remain stable while the Nix DSL evolves. The execution layer requires no knowledge of how the Lab was constructed.

### Host Tools Bundling

Labs include a set of statically-linked host tools in `host-tools/` for bootstrapping execution on machines without Nix. These are content-addressed and include:

- `coreutils`, `findutils`, `sed`, `grep`, `bash` -- basic shell utilities
- `jq` -- JSON processing for metadata
- `tar`, `pigz` (aliased as `gzip`) -- archive handling
- `bubblewrap` -- container-less sandboxing
- `openssh`, `rsync` -- remote sync and transfer

This allows the Lab to be self-contained: you can copy the `result` directory to any compatible Linux machine and execute experiments without requiring Nix on the target.

### Build-Time Validation

Every stage script undergoes automatic validation during the Nix build:

1. **ShellCheck** lints for common Bash issues
2. **OSH** (Oils for Unix) parses the script into an AST
3. **Dependency analysis** extracts all external command invocations and verifies each one exists in `$PATH` (populated by `runDependencies`)

This catches missing dependencies at build time rather than discovering them at runtime on a remote cluster.

## Type System

RepX employs strongly-typed enumerations to enforce valid configurations:

**StageType**: Classifies job execution semantics
- `Simple`: Standard single-execution job
- `ScatterGather`: Parallel fan-out pattern
- `Worker`: Individual scatter partition
- `Gather`: Aggregation of worker outputs

**SchedulerType**: Execution backend selection
- `Local`: Direct execution on target host
- `Slurm`: SLURM workload manager integration

## Error Handling

Errors are categorized into two domains:

**ConfigError**: Configuration and I/O issues
- File system operations
- Metadata parsing (JSON, TOML)
- Path resolution
- Lab validation

**DomainError**: Logical and semantic errors
- Job/run resolution failures
- Ambiguous identifiers
- Invalid output specifications
- Execution mode conflicts

## Logging Infrastructure

RepX uses the `tracing` crate for structured logging. Log levels are configurable via:

- The `REPX_LOG_LEVEL` environment variable
- CLI verbosity flags (`-v`, `-vv`, etc.)
- Configuration file settings

Log output includes timestamps and is written to rotating log files in the XDG cache directory.
