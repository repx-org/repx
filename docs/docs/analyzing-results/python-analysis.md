# Python Analysis

Analyzing reproducible experiments often requires locating specific output files buried within hashed directory structures. `repx-py` abstracts this complexity, allowing users to query jobs by name, parameters, or dependency relationships and retrieve their outputs as standard Python objects or pandas DataFrames.

## Installation

`repx-py` is available as a flake package. Include it in your project's development shell:

```nix
# flake.nix
{
  inputs.repx.url = "github:repx-org/repx";
  
  outputs = { self, nixpkgs, repx }: {
    devShells.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {
      packages = [
        repx.packages.x86_64-linux.repx-py
      ];
    };
  };
}
```

Or build it directly:

```bash
nix build github:repx-org/repx#repx-py
```

## Loading an Experiment

The `Experiment` class is the entry point. It loads the Lab metadata and allows you to query runs and jobs.

```python
from repx_py import Experiment

# Initialize from the built lab directory
exp = Experiment(lab_path="./result")

print(f"Loaded experiment with {len(exp.jobs())} total jobs")
```

## Querying Jobs

The `JobCollection` interface allows you to filter jobs based on their metadata and parameters.

### 1. Basic Filtering

```python
# Get all jobs in the 'simulation' run
jobs = exp.jobs().filter(name__startswith="simulation")

# Filter by exact parameter match
jobs = exp.jobs().filter(param_model="resnet50")
```

### 2. Advanced Filtering

Supported operators:
*   `__startswith`
*   `__endswith`
*   `__contains`
*   `param_<NAME>`: Search within effective parameters.

```python
# Find jobs where learning rate is 0.01 AND model name contains 'net'
target_jobs = exp.jobs().filter(
    param_learning_rate=0.01,
    param_model__contains="net"
)
```

### 3. Converting to DataFrame

You can convert a collection of jobs into a Pandas DataFrame to inspect their metadata and parameters in tabular format.

```python
df = target_jobs.to_dataframe()
print(df)
# Output:
#                                   name  param_learning_rate param_model
# job_id                                                                 
# 8f2a... simulation.train-model-batch-1                 0.01      resnet
```

## Accessing Results

Once you have a `JobView` (by iterating over a collection or getting a specific ID), you can access its outputs.

```python
for job in target_jobs:
    print(f"Analyzing Job: {job.id}")

    # 1. Get absolute path to an output file
    log_path = job.get_output_path("run.log")

    # 2. Load CSV data directly
    #    (Assumes the stage defined an output named "metrics.csv")
    metrics_df = job.load_csv("metrics.csv")
    print(metrics_df.describe())

    # 3. Load JSON data
    config = job.load_json("config.json")
```

## Effective Parameters

RepX resolves the "effective parameters" for every job by tracing values inherited from upstream dependencies. This means you always know exactly what configuration produced a result, even if parameters were defined in a producer stage.

```python
# Access resolved parameters
print(job.effective_params)
```

## Analysis within a Pipeline

When running an analysis stage *inside* a RepX pipeline, you don't have access to the full Lab directory (because it's being built!). Instead, you use the `from_run_metadata` factory method.

**Inside `analysis.py`:**
```python
import argparse
from repx_py import Experiment

parser = argparse.ArgumentParser()
parser.add_argument("--meta", help="Path to input run metadata")
parser.add_argument("--store", help="Path to artifact store base")
args = parser.parse_args()

# Load context from the specific upstream run
exp = Experiment.from_run_metadata(args.meta, args.store)

# Now you can query the upstream jobs as usual
jobs = exp.jobs()
```
