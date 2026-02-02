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

## Best Practices

1.  **Use absolute paths in `$out`**: Always define outputs as `$out/filename`.
2.  **Quote paths**: Input paths may contain spaces. Always use `"${inputs[name]}"`.
3.  **Sanitize Parameters**: When injecting parameters into Bash, use `toString` or proper escaping if they contain special characters.
