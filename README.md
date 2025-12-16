# repx-rs

**repx-rs** is the execution engine for the RepX framework. Written in Rust, it provides the command-line interface (CLI) and Terminal User Interface (TUI) for submitting, orchestrating, and monitoring experiments defined via `repx-nix`.

## Overview

The `repx-runner` binary handles the complexity of executing jobs across different environments. It supports local execution for development and remote execution via SSH for production HPC clusters. It abstracts away scheduler interaction (SLURM) and containerization (Bubblewrap, Docker, Podman).

## Components

This workspace contains the following crates:

*   **repx-runner:** The primary CLI binary.
*   **repx-tui:** A terminal-based dashboard for monitoring job status and logs.
*   **repx-client:** The core logic for communicating with execution targets.
*   **repx-executor:** Handles the low-level process execution and container runtime wrapping.
*   **repx-core:** Shared configuration and data models.

## Installation

### Prerequisites
*   Rust (latest stable)
*   `pkg-config`
*   OpenSSL development headers

### Build
```bash
cargo build --release
```
The binary will be located at `target/release/repx-runner`.

## Configuration

Configuration is managed via `config.toml`, typically located at `~/.config/repx/config.toml`.

```toml
submission_target = "cluster"

[targets.local]
base_path = "/home/user/repx-store"
default_scheduler = "local"

[targets.cluster]
address = "user@hpc-login-node"
base_path = "/scratch/user/repx-store"
default_scheduler = "slurm"
# Optional: Fast local storage for container caching
node_local_path = "/tmp/user/repx" 

[targets.cluster.slurm]
execution_types = ["podman", "native"]
```

## Usage

### Running Experiments

To submit a run defined in your Lab:

```bash
# Run locally
repx-runner run simulation-run --target local

# Run on a remote SLURM cluster
repx-runner run simulation-run --target cluster --scheduler slurm
```

### Monitoring (TUI)

The TUI provides real-time status updates, log streaming, and job management.

```bash
repx-tui --lab ./result
```

### Garbage Collection

RepX manages a content-addressable store. Use the GC command to clean up unused artifacts and outputs.

```bash
repx-runner gc --target cluster
```

## Features

*   **Multi-Target Support:** Seamless switching between local machines and remote clusters.
*   **Orchestration:** Handles dependency resolution and job submission order.
*   **Container Abstraction:** Supports running jobs natively or inside containers (Podman, Docker, Bubblewrap) without changing the experiment definition.
*   **Resilience:** Idempotent execution logic prevents re-running successful jobs.

