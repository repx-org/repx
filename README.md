<div align="center">
  <img src="docs/static/img/logo.svg" height="150" alt="RepX Logo" />
  <h1>RepX</h1>
</div>

RepX is a framework for reproducible High-Performance Computing (HPC) experiments. It leverages [Nix](https://nixos.org) to guarantee environment reproducibility across diverse execution targets, from local workstations to SLURM-based clusters.

## Documentation

Comprehensive documentation is available in the [`docs/`](./docs/docs) directory.

*   **Getting Started**: [Quickstart](./docs/docs/getting-started/quickstart.md) | [Installation](./docs/docs/getting-started/installation.md)
*   **User Guide**: [Defining Experiments](./docs/docs/user-guide/defining-experiments.md)
*   **Reference**: [CLI](./docs/docs/reference/cli-reference.md) | [Python API](./docs/docs/reference/python-api.md) | [Nix DSL](./docs/docs/reference/nix-functions.md)

## Components

The framework is organized into three primary layers:

*   **[repx-nix](./nix)**: The Definition Layer. A Nix library for defining stages and pipelines.
*   **[repx-rs](./crates)**: The Execution Layer. A Rust-based runtime for orchestrating jobs.
*   **[repx-py](./python)**: The Analysis Layer. A Python library for querying results.
