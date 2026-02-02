# Dependencies

Dependencies in RepX are primarily data-driven. When one stage consumes the output of another, a dependency is established. RepX handles two types of dependencies: **Stage Dependencies** (within a pipeline) and **Run Dependencies** (between separate runs).

## Stage Dependencies

Within a single pipeline, dependencies determine the execution order (DAG).

```nix
consumer = repx.callStage ./consumer.nix [
  # Explicit dependency on 'producer'
  producer
];
```

The runtime ensures `producer` completes successfully before `consumer` starts.

## Run Dependencies

You can also define dependencies between entire Runs. This is useful for separating long-running simulations from quick analysis steps.

<div align="center">
  <img src="/images/simple-topology.svg" alt="Run Dependencies" />
</div>

```nix
runs = rec {
  simulation = repx-lib.callRun ./run-simulation.nix [ ];

  analysis = repx-lib.callRun ./run-analysis.nix [
    [ simulation "soft" ]
  ];
};
```

### Dependency Types

*   **Hard Dependency (`"hard"`)**: The dependent run waits for the upstream run to complete *successfully*. If the upstream fails, the dependent run is not executed.
*   **Soft Dependency (`"soft"`)**: The dependent run waits for the upstream run to finish (success or failure). This is useful for analysis jobs that might want to inspect partial results or logs even if the simulation crashed.

## Software Dependencies

At the stage level, software dependencies are handled by Nix.

```nix
{ pkgs }:
{
  # ...
  runDependencies = [
    pkgs.python3
    pkgs.ffmpeg
    pkgs.imagemagick
  ];
  # ...
}
```

These packages are guaranteed to be available in the `PATH` of the executing job, regardless of where it runs (local, SLURM, container).
