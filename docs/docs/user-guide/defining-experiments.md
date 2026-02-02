# Defining Experiments

RepX uses **Nix** as a configuration language to define reproducible experiments. This approach treats experiments as code, allowing you to version control, modularize, and share your scientific workflows.

## The RepX Model

A RepX experiment is composed of three main concepts:

1.  **Stages**: The atomic units of work (e.g., "generate data", "train model", "plot results"). A stage defines *what* to run.
2.  **Pipelines**: A directed graph of stages. Pipelines define *how* data flows between stages.
3.  **Runs**: A concrete instantiation of a pipeline with specific parameters.

## Directory Structure

A typical RepX project structure looks like this:

```
my-experiment/
├── flake.nix             # Entry point (The Lab definition)
├── nix/
│   ├── lab.nix           # High-level experiment organization
│   ├── runs/             # Run definitions
│   │   └── simulation.nix
│   └── stages/           # Individual stage definitions
│       ├── generator.nix
│       └── analysis.nix
└── src/                  # Your actual application code (Python, C++, etc.)
    ├── generate.py
    └── analyze.py
```

## The "Lab"

The top-level object in RepX is the **Lab**. The Lab is a collection of all your defined runs. It is exported from your `flake.nix` and built using `nix build`.

**nix/lab.nix**:
```nix
{ pkgs, repx-lib, ... }:
repx-lib.mkLab {
  inherit pkgs;
  
  # specific attributes...
  runs = {
    simulation = import ./runs/simulation.nix;
    analysis   = import ./runs/analysis.nix;
  };
}
```

## Next Steps

*   **[Stages](./stages.md)**: Learn how to define individual computational steps.
*   **[Pipelines](./pipelines.md)**: Learn how to connect stages into a workflow.
*   **[Parameters](./parameters.md)**: Learn how to inject and sweep parameters.
