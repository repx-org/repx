# RepX: Reproducible HPC Experiment Framework

**RepX** is a framework designed to bring Nix-based reproducibility to High-Performance Computing (HPC) workflows. It decouples the definition of experiments from their execution, ensuring that scientific pipelines are portable, deterministic, and easily analyzable.

## Ecosystem Overview

The framework consists of three primary components:

1.  **[repx-nix](https://github.com/repx-org/repx-nix):** The Definition Layer. A Nix library for defining stages, pipelines, and parameter sweeps. It builds the static "Lab" artifact.
2.  **[repx-rs](https://github.com/repx-org/repx-rs):** The Execution Layer. A Rust-based runtime (CLI/TUI) that synchronizes the Lab to execution targets (local or SSH/SLURM) and orchestrates job execution.
3.  **[repx-py](https://github.com/repx-org/repx-py):** The Analysis Layer. A Python library for querying results and metadata from the structured output store.

## Workflow

1.  **Define:** Create a `flake.nix` using `repx-nix` to describe your experiment topology and software dependencies.
2.  **Build:** Run `nix build` to generate the "Lab" directory. This step performs static analysis on your scripts and locks dependencies.
3.  **Run:** Use `repx-runner` to submit the Lab to a target (e.g., your laptop or a supercomputer). The runner handles data transfer, job scheduling, and containerization.
4.  **Analyze:** Use `repx-py` in Jupyter notebooks or scripts to load data from the results, agnostic of the directory structure.

## Examples

This repository contains integration examples demonstrating standard patterns.

*   **`examples/simple`:** A basic linear pipeline producing data and calculating a checksum. Demonstrates basic stage definition and Python analysis integration.
*   **`examples/param-sweep`:** Demonstrates how to define parameter sweeps using `repx-nix`, execute them in parallel, and aggregate the results in a downstream plotting stage.

## Getting Started

To run the simple example:

1.  Navigate to `examples/simple`.
2.  Build the experiment:
    ```bash
    nix build
    ```
3.  Run the experiment locally:
    ```bash
    repx-runner run simulation-run --lab ./result
    ```
4.  View results via the TUI:
    ```bash
    repx-tui --lab ./result
    ```

## Contributing

We welcome contributions. Please see the specific repository for contribution guidelines related to the Nix DSL, Rust runtime, or Python client.

