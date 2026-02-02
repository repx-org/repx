# Pipelines

Pipelines connect stages into a Directed Acyclic Graph (DAG). They define the flow of data: outputs from one stage become inputs for the next.

## Defining a Pipeline

Pipelines are defined using the `repx.mkPipe` function. This function takes an attribute set where each key is a stage name and each value is a stage instantiation.

**pipeline.nix**:
```nix
{ repx }:
repx.mkPipe rec {
  # 1. Instantiate the producer
  producer = repx.callStage ./stages/producer.nix [ ];

  # 2. Instantiate the consumer, depending on producer
  consumer = repx.callStage ./stages/consumer.nix [
    producer
  ];
}
```

## Dependency Injection

Dependencies are passed as the second argument to `repx.callStage`. RepX supports two ways to map outputs to inputs.

### 1. Implicit Mapping (Name Matching)

If you pass a stage object directly, RepX attempts to match output names to input names automatically.

*   **Producer Output**: `data.csv`
*   **Consumer Input**: `data.csv`

```nix
consumer = repx.callStage ./consumer.nix [ producer ];
```

If the names match, the link is created. If they don't match, the build will fail with a clear error message.

### 2. Explicit Mapping

If the names differ, use a list to define the mapping explicitly: `[ stage source_output target_input ]`.

*   **Producer Output**: `raw_data`
*   **Consumer Input**: `input_file`

```nix
consumer = repx.callStage ./consumer.nix [
  [ producer "raw_data" "input_file" ]
];
```

## Inter-Run Dependencies (The "First Stage" Rule)

A RepX Lab often consists of multiple Runs (e.g., a simulation run and an analysis run). The analysis run depends on the simulation run.

If a stage is the **first stage** in a pipeline (i.e., it has no dependencies on other stages within the same pipeline), it acts as the bridge for **Inter-Run Dependencies**.

### Mandatory Inputs

If your Run depends on another Run (e.g., `analysis` depends on `simulation`), the first stage of `analysis` **MUST** declare specific inputs to receive that data.

1.  **Metadata Input**: `metadata__<RunName>`
    *   Receives the path to the metadata of the upstream run.
2.  **Base Store Input**: `store__base` (If using shared storage)
    *   Receives the base path of the artifact store.

**Example: Analysis Pipeline**

```nix
# This stage is the first in the analysis pipeline.
# It depends on the 'simulation' run.
{ pkgs }:
{
  pname = "analyzer";
  
  inputs = {
    # REQUIRED: Receive metadata from the 'simulation' run
    "metadata__simulation" = "";
    
    # REQUIRED: Receive the base store path
    "store__base" = "";
  };
  
  outputs = { "report.pdf" = "$out/report.pdf"; };
  
  run = { inputs, ... }: ''
    # Use the repx-py library to query the simulation results
    python3 analyze.py \
      --meta "${inputs.metadata__simulation}" \
      --store "${inputs.store__base}"
  '';
}
```

## Visualization

You can visualize the connections in your pipeline using the CLI:

```bash
repx viz --lab ./result
```

This generates a graph showing how data flows through your stages, which is invaluable for debugging complex topologies.
