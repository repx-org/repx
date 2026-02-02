# Core Concepts

RepX is a framework designed to bring Nix-based reproducibility to High-Performance Computing (HPC) workflows. It decouples the definition of experiments from their execution.

## The RepX Ecosystem

The framework consists of three primary components:

1.  **repx (The Definition Layer):** A Nix library for defining stages, pipelines, and parameter sweeps. It builds the static "Lab" artifact.
2.  **repx CLI (The Execution Layer):** A Rust-based unified CLI (`repx`) that synchronizes the Lab to execution targets (local or SSH/SLURM), orchestrates job execution, and visualizes experiment topologies.
3.  **repx-py (The Analysis Layer):** A Python library for querying results and metadata from the structured output store.

## Key Terminology

### Stage
An individual unit of work (e.g., a script) with defined inputs, outputs, parameters, and software dependencies.

### Pipeline
A sequence of stages connected by data dependencies. The output of one stage becomes the input of another.

### Run
A parameterized instance of a pipeline. You can have multiple runs for the same pipeline structure with different parameter configurations.

### The Lab
A self-contained directory structure (produced by `nix build`) containing all experiment metadata, build scripts, and dependency closures. It is the "executable" artifact of your experiment.

## Workflow

1.  **Define:** Create a `flake.nix` using the `repx` library to describe your experiment topology and software dependencies.
2.  **Build:** Run `nix build` to generate the "Lab" directory.
3.  **Visualize:** Use `repx viz` to generate a graph of the experiment topology.
    
    <div align="center">
      <img src="/images/simple-topology.svg" alt="Experiment Topology" />
    </div>

4.  **Run:** Use `repx run` to submit the Lab to a target (e.g., your laptop or a supercomputer).
5.  **Analyze:** Use `repx-py` in Jupyter notebooks to load data from the results.
