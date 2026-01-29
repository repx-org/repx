# repx-nix

**repx-nix** provides the core Nix library and Domain Specific Language (DSL) for the RepX framework. It facilitates the definition of reproducible High-Performance Computing (HPC) experiments, managing the dependency graph, environment isolation, and the generation of the portable "Lab" artifact.

## Overview

This repository contains the Nix functions required to define:
*   **Stages:** Individual units of work with defined inputs, outputs, and software dependencies.
*   **Pipelines:** Sequences of stages connected by data dependencies.
*   **Runs:** Parameterized instances of pipelines.
*   **The Lab:** A self-contained directory structure containing all experiment metadata, build scripts, and dependency closures.

## Features

*   **Reproducible Environments:** Leveraging Nix to ensure bit-for-bit reproducibility of software environments across different HPC clusters.
*   **Automatic Dependency Analysis:** Includes static analysis tools (Python/bashlex) to detect missing shell dependencies in stage scripts during the build phase.
*   **Scatter-Gather Support:** Native abstractions for defining parallel workload distributions.
*   **Metadata Generation:** Automatically generates JSON metadata describing the experiment topology for consumption by the runtime and analysis layers.

## Usage

This library is intended to be used as a flake input in your experiment repository.

### Importing the Library

In your `flake.nix`:

```nix
inputs {
  nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
  repx-nix.url = "github:repx-org/repx-nix";
}
```

### Defining a Stage

Stages are defined using `repx.callStage`. A stage declares its I/O contract and the script to execute.

```nix
{ pkgs }:
{
  pname = "data-generator";
  version = "1.0";

  # Define file outputs relative to $out
  outputs = {
    "data.csv" = "$out/data.csv";
  };

  # Define parameters with default values
  params = {
    seed = 42;
  };

  # Nix packages required in the $PATH
  runDependencies = [ pkgs.python3 ];

  # The execution script
  run = { outputs, params, ... }: ''
    python3 generate_data.py --seed ${toString params.seed} --output "${outputs."data.csv"}"
  '';
}
```

### Defining a Pipeline

Pipelines wire stages together using `repx.mkPipe`. Dependencies can be explicit (passing stage objects) or implicit via input/output naming conventions.

```nix
{ repx }:
repx.mkPipe {
  generator = repx.callStage ./stage-generator.nix [ ];
  
  # The consumer depends on the generator
  consumer = repx.callStage ./stage-consumer.nix [
    [ generator "data.csv" "input_file" ]
  ];
}
```

## Architecture

The library produces a derivation called the **Lab**. The Lab follows a strict schema:

*   `/lab`: Contains the manifest and top-level metadata.
*   `/jobs`: Contains build scripts and dependency closures for every defined job.
*   `/revision`: Contains run-specific metadata.
*   `/host-tools`: Static binaries required for bootstrapping execution on remote targets.

