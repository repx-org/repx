# Simple Pipeline Example

This example demonstrates a basic linear pipeline producing data and calculating a checksum. It also shows how to integrate `repx-py` for analysis.

Location: `examples/simple`

## Structure

The experiment is defined in `flake.nix`, which imports the lab definition from `nix/lab.nix`.

<div align="center">
  <img src="/images/simple-topology.svg" alt="Simple Pipeline Topology" />
</div>

```nix
# nix/lab.nix
{
  pkgs,
  repx-lib,
  gitHash,
}:

repx-lib.mkLab {
  inherit pkgs gitHash repx-lib;
  runs = rec {
    # Run 1: Produce numbers and calculate a sum
    simulation = repx-lib.callRun ./run-simulation.nix [ ];

    # Run 2: Analyze the results of Run 1 using repx-py
    analysis = repx-lib.callRun ./run-analysis.nix [
      [
        simulation
        "soft"
      ]
    ];
  };
}
```

The lab defines two runs:
1.  `simulation`: Runs the data production and processing pipeline.
2.  `analysis`: Depends on `simulation` (soft dependency) and runs analysis scripts.

## Running the Example

1.  **Build the Lab:**
    ```bash
    nix build
    ```

2.  **Visualize (Optional):**
    You can inspect the generated topology using the TUI or the `viz` command.
    
    <div align="center">
      <img src="/images/simple-tui.png" alt="Simple Pipeline TUI" />
    </div>

3.  **Run the Simulation:**
    ```bash
    repx run simulation --lab ./result
    ```

3.  **Run the Analysis:**
    ```bash
    repx run analysis --lab ./result
    ```
