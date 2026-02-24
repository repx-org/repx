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
let
  simulation = repx-lib.callRun ./runs/simulation.nix [];
  analysis   = repx-lib.callRun ./runs/analysis.nix [ simulation ];
in
repx-lib.mkLab {
  inherit pkgs repx-lib;
  gitHash = self.rev or self.dirtyRev or "unknown";
  lab_version = "1.0.0";

  runs = {
    inherit simulation analysis;
  };
}
```

The `mkLab` function requires:
- **`gitHash`**: Git commit hash for provenance tracking.
- **`lab_version`**: A version string for your experiment.
- **`runs`**: A set of run placeholders created by `callRun`.

Runs are connected using `callRun`, which accepts a path to a run definition file and a list of dependencies. See the [Nix Functions Reference](../reference/nix-functions.md#repx-libcallrun) for full details.

## Run Groups

For large experiments, you can organize runs into named groups:

```nix
repx-lib.mkLab {
  inherit pkgs repx-lib;
  gitHash = self.rev or self.dirtyRev or "unknown";
  lab_version = "1.0.0";

  runs = {
    inherit preprocess training evaluation visualization;
  };

  groups = {
    ml-pipeline = [ preprocess training evaluation ];
    reporting = [ visualization ];
  };
}
```

Groups are organizational only -- they don't affect execution. Use `repx list groups` to inspect them.

## Next Steps

*   **[Stages](./stages.md)**: Learn how to define individual computational steps.
*   **[Pipelines](./pipelines.md)**: Learn how to connect stages into a workflow.
*   **[Parameters](./parameters.md)**: Learn how to inject and sweep parameters.
*   **[Advanced Patterns](../examples/advanced-patterns.md)**: Run groups, resource hints, directory scanning, and more.
