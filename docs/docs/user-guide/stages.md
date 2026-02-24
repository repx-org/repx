# Stages

A **Stage** is the atomic unit of execution in a RepX experiment. It represents a single script or command that transforms inputs into outputs.

RepX provides two primary stage types:
1.  **Simple Stage**: A standard single-job execution.
2.  **Scatter-Gather Stage**: A parallel map-reduce pattern.

Stages are defined as Nix functions that accept `pkgs` and return an attribute set.

## Simple Stage

A simple stage runs a single script.

### Schema

```nix
{ pkgs }:
{
  pname = "data-generator";
  version = "1.0";
  
  # 1. Inputs: Map of input identifiers to default values.
  # These become available as keys in the $inputs associative array in the script.
  inputs = {
    "config_file" = "defaults.json";
    "seed_file"   = "$out/seed.txt"; # Can reference outputs of upstream stages
  };

  # 2. Outputs: Map of output identifiers to file paths.
  # Use $out as the base directory.
  outputs = {
    "data_csv" = "$out/data.csv";
    "logs"     = "$out/run.log";
  };
  
  # 3. Parameters: Default values for parameters.
  # These are injected into the params attribute set.
  params = {
    size = 100;
    method = "uniform";
  };

  # 4. Dependencies: Nix packages to include in $PATH.
  runDependencies = [ pkgs.python3 pkgs.jq ];

  # 5. The Execution Script.
  # It receives { inputs, outputs, params, pkgs } as arguments.
  run = { inputs, outputs, params, ... }: ''
    echo "Running generator with size ${toString params.size}"
    
    # Access inputs via the bash array
    # Note: In the script, utilize "''${inputs[config_file]}"
    
    python3 generate.py \
      --size ${toString params.size} \
      --config "''${inputs[config_file]}" \
      --output "${outputs.data_csv}"
  '';
}
```

### Execution Environment

RepX runs your script in a tightly controlled environment:

*   **`$PATH`**: Contains *only* the packages listed in `runDependencies`.
*   **`$out`**: The directory where outputs must be written. RepX clears this directory before every run.
*   **`$inputs`**: A Bash associative array containing the absolute paths to input files.
    *   Usage: `"${inputs[input_name]}"`
*   **`params`**: Parameters are injected directly into the script string (since the script is a Nix string).

## Scatter-Gather Stage

A scatter-gather stage automatically scales tasks across your compute resources. It consists of three sub-stages:
1.  **Scatter**: Generates a list of work items.
2.  **Worker**: Executes for each work item in parallel.
3.  **Gather**: Aggregates the results.

### Schema

```nix
{ pkgs }:
{
  pname = "parameter-sweep";
  
  # Parameters applicable to the whole group
  params = { chunks = 10; };

  # --- 1. Scatter ---
  scatter = {
    inputs = { "data" = "source.csv"; };
    outputs = {
      # MANDATORY: The JSON file containing the list of work items
      "work__items" = "$out/work_items.json";
      # MANDATORY: Schema/Example of what a single worker receives
      "worker__arg" = { index = 0; chunk_id = ""; };
    };
    run = { inputs, outputs, params, ... }: ''
      # Must generate a JSON list of objects matching worker__arg
      jq -n '[range(${toString params.chunks}) | {index: ., chunk_id: "c\(.)"}]' \
        > "${outputs.work__items}"
    '';
  };

  # --- 2. Worker ---
  worker = {
    inputs = {
      # MANDATORY: Receives one item from the scatter list
      "worker__item" = ""; 
    };
    outputs = {
      "result" = "$out/partial.csv";
    };
    run = { inputs, outputs, ... }: ''
      # worker__item is a JSON file containing the single item object
      idx=$(jq -r .index < "${inputs.worker__item}")
      echo "Processing $idx" > "${outputs.result}"
    '';
  };

  # --- 3. Gather ---
  gather = {
    inputs = {
      # MANDATORY: Receives a JSON list of all worker output paths
      "worker__outs" = "[]";
    };
    outputs = {
      "final" = "$out/final.csv";
    };
    run = { inputs, outputs, ... }: ''
      # worker__outs is a JSON list of objects. Each object has keys matching worker outputs.
      # e.g. [{"result": "/path/to/worker1/partial.csv"}, ...]
      
      cat "${inputs.worker__outs}" | jq -r '.[].result' | xargs cat > "${outputs.final}"
    '';
  };
}
```

## Resource Hints

Stages can declare resource requirements for SLURM scheduling. These are optional and have no effect on local execution.

```nix
{ pkgs }:
{
  pname = "gpu-training";
  
  resources = {
    mem = "32G";           # Memory (K, M, G, T suffixes)
    cpus = 8;              # CPU count
    time = "12:00:00";     # Wall time (HH:MM:SS)
    partition = "gpu";     # SLURM partition
    sbatch_opts = [ "--gres=gpu:1" ];  # Extra sbatch flags
  };

  # ...
}
```

Resource hints are automatically **merged from upstream dependencies**: `mem`, `cpus`, and `time` take the maximum across all inputs. The stage's own `partition` and `sbatch_opts` take precedence.

For scatter-gather stages, each sub-stage (`scatter`, `worker`, `gather`) can have its own `resources` attribute.

See the [Nix Functions Reference](../reference/nix-functions.md#resource-hints) for full details.

## Dynamic Attributes

The `pname`, `inputs`, `outputs`, and `resources` attributes can be **functions** that accept `{ params }` for dynamic resolution:

```nix
{ pkgs }:
{
  pname = { params }: "process-${params.model}";
  
  outputs = { params }: {
    "result" = "$out/result-${params.model}.csv";
  };

  resources = { params }: {
    mem = if params.dataset_size > 10000 then "64G" else "8G";
  };

  # ...
}
```

This allows stages to adapt their name, I/O structure, and resource requirements based on parameter values. See [Advanced Patterns](../examples/advanced-patterns.md#dynamic-stages) for more examples.

## Script Validation

RepX automatically validates stage scripts at **build time**:

1.  **ShellCheck** lints the script for common Bash issues.
2.  **OSH** (Oils for Unix) parses the script into an AST.
3.  **Dependency analysis** extracts all external command invocations and verifies each command exists in `$PATH` (populated by `runDependencies`).

If your script calls a command not provided by `runDependencies`, the Nix build fails with an error listing the missing commands. This catches dependency issues early.

## Script Execution Contract

When RepX executes your stage script at runtime:

1.  **Shell settings**: `set -euxo pipefail` -- the script fails on any error, undefined variable, or pipe failure.
2.  **Input readiness**: RepX polls for all input files with a 30-second timeout (handling async filesystem syncs on networked storage).
3.  **Output cleanup**: `$out` is cleared before each run (preserving `slurm-*.out` files).
4.  **Working directory**: Set to `$out`.

## Best Practices

1.  **Use absolute paths in `$out`**: Always define outputs as `$out/filename`.
2.  **Quote paths**: Input paths may contain spaces. Always use `"${inputs[name]}"`.
3.  **Sanitize Parameters**: When injecting parameters into Bash, use `toString` or proper escaping if they contain special characters.
4.  **Declare all dependencies**: List every external command your script uses in `runDependencies`. The build-time validator will catch omissions.
5.  **Use resource hints**: When targeting SLURM, declare resource requirements so the scheduler can allocate resources efficiently.
