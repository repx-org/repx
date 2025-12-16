# repx-py

**repx-py** is the Python client library for the RepX framework. It provides data scientists and researchers with a programmatic interface to query experiment metadata, resolve file paths within the content-addressable store, and load results into analysis environments.

## Overview

Analyzing reproducible experiments requires locating specific output files buried within hashed directory structures. `repx-py` abstracts this complexity, allowing users to query jobs by name, parameters, or dependency relationships and retrieve their outputs as standard Python objects or pandas DataFrames.

## Installation

This package is typically installed via Nix within a `repx-nix` environment, but can be installed manually for development.

```bash
pip install .
```

## Core Concepts

*   **Experiment:** Represents the root of a RepX Lab.
*   **JobView:** A read-only interface to a specific job execution, providing access to its parameters and output paths.
*   **JobCollection:** A filterable list of jobs.

## Usage Example

### Loading Data

```python
from repx_py import Experiment

# Initialize the experiment from the lab directory
exp = Experiment("./result")

# Filter jobs by name and parameters
jobs = exp.jobs().filter(
    name__startswith="simulation",
    param_learning_rate=0.01
)

# Iterate through jobs and load results
for job in jobs:
    print(f"Job ID: {job.id}")
    print(f"Effective Parameters: {job.effective_params}")
    
    # Load a CSV output directly into a pandas DataFrame
    df = job.load_csv("metrics.csv")
    print(df.head())
    
    # Resolve the absolute filesystem path for a specific output
    plot_path = job.get_output_path("plot.png")
    print(f"Plot located at: {plot_path}")
```

### Parameter Tracing

The library can resolve the "effective parameters" of a job by traversing its dependency graph, collecting parameters from upstream producers.

```python
# Trace effective parameters for all jobs
params = exp.effective_params
```

## Visualization Tools

The package includes a CLI tool `repx-viz` to generate Graphviz topology diagrams of the experiment.

```bash
repx-viz ./result -o topology.png
```

