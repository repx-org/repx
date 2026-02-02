# Quickstart

This guide will walk you through running your first RepX experiment using the `simple` example provided in the repository. You will learn how to build a "Lab", visualize the experiment topology, run it locally, and inspect the results.

## Prerequisites

- Ensure you have [installed RepX](./installation.md).
- Ensure the `repx` binary is in your PATH.

## 1. Get the Example

If you haven't cloned the repository, you can fetch the example code. For this guide, we assume you are inside the `repx` repository, but these steps apply to any RepX project.

```bash
# Navigate to the simple example
cd examples/simple
```

## 2. Build the Experiment ("The Lab")

In RepX, an experiment definition is compiled into a **Lab**. The build process performs:
1.  **Static Analysis**: Checks your scripts for syntax errors (e.g., using `shellcheck` for Bash).
2.  **Dependency Locking**: Resolves all software dependencies via Nix.
3.  **Graph Construction**: Builds the dependency graph of all stages.

Run `nix build` to generate the Lab. By default, this creates a `result` symlink containing the lab definition.

```bash
nix build
# Defines the Lab in ./result
```

## 3. Visualize the Topology (Optional)

Before running, it's helpful to visualize what will happen. Use `repx viz` to generate a graph of the experiment topology. This shows how data flows between stages.

```bash
repx viz --lab ./result
# Generates topology.png by default
```

*Open `topology.png` to see the graph.*

## 4. Run the Experiment

Use `repx run` to execute the experiment. You must specify:
1.  The **Run Name**: The example defines a run named `simulation`.
2.  The **Lab Path**: Point to the `./result` directory we just built.

```bash
repx run simulation --lab ./result
```

RepX will:
-   Verify that all dependencies are met.
-   Execute stages in parallel where possible.
-   Cache results (re-running this command will simply report "Cached" for completed stages).

## 5. Monitor with the TUI

For larger experiments, you can monitor progress in real-time using the Terminal User Interface (TUI).

```bash
repx tui --lab ./result
```

Key features of the TUI:
-   **Job Status**: See which stages are Pending, Running, Failed, or Completed.
-   **Logs**: Select a job and press `Enter` to view its stdout/stderr.
-   **Resource Usage**: Monitor CPU and Memory usage (if supported by the backend).

## 6. Analyze Results

Once the run is complete, you can analyze the results using the Python API. RepX organizes outputs in a structured format that is easy to query.

```python
from repx_py import Experiment

# Load the experiment
exp = Experiment(lab_path="./result")

# Get the 'simulation' run
run = exp.runs["simulation"]

# List all jobs
print(run.jobs.df)
```

See [Analyzing Results](../analyzing-results/python-analysis.md) for a deep dive into the Python API.
