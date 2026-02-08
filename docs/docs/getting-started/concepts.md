# Core Concepts

RepX provides reproducible experiment execution on High-Performance Computing infrastructure. The framework separates experiment definition from execution, enabling portable workflows across heterogeneous compute environments.

## Architecture Overview

RepX comprises three distinct layers:

| Layer | Component | Function |
|-------|-----------|----------|
| Definition | Nix library | Experiment specification, dependency resolution |
| Execution | Rust CLI | Orchestration, synchronization, job management |
| Analysis | Python library | Result querying, metadata access |

## Terminology

### Stage

A discrete computational unit with explicit interfaces:

- **Inputs**: Data dependencies from upstream stages
- **Outputs**: Produced artifacts consumed by downstream stages
- **Parameters**: Configuration values affecting execution
- **Environment**: Software dependencies captured as Nix closures

### Pipeline

A directed acyclic graph (DAG) of stages connected by data flow dependencies. Stage outputs map to downstream stage inputs.

### Run

A parameterized pipeline instantiation. Multiple runs may share pipeline structure with varying parameter configurations.

### Lab

The build artifact produced by `nix build`. A Lab encapsulates:

- Experiment DAG structure (metadata JSON)
- Job executables and dependency closures
- Container images (when applicable)
- Host tools for target bootstrapping

## Workflow

### 1. Define

Specify experiment structure using the `repx` Nix library:

```nix
{
  outputs = { self, repx, ... }: {
    packages.x86_64-linux.default = repx.lib.runs2Lab [
      # Run definitions
    ];
  };
}
```

### 2. Build

Generate the Lab artifact:

```bash
nix build
```

### 3. Visualize

Inspect experiment topology:

```bash
repx viz --format svg
```

<div align="center">
  <img src="/images/simple-topology.svg" alt="Experiment Topology" />
</div>

### 4. Execute

Submit to an execution target:

```bash
repx run <run_name> --lab ./result [--target <target>]
```

### 5. Analyze

Query results programmatically:

```python
from repx_py import Lab

lab = Lab.from_path("./result")
for job in lab.jobs():
    print(job.outputs)
```

## Execution Model

### Scheduling

RepX supports two scheduler backends:

| Scheduler | Use Case |
|-----------|----------|
| `local` | Direct execution with concurrency control |
| `slurm` | HPC cluster submission via SLURM |

### Runtime Environments

Jobs execute in isolated environments:

| Runtime | Description |
|---------|-------------|
| Native | Direct process on host system |
| Bwrap | Bubblewrap namespace isolation |
| Docker | Docker container |
| Podman | Podman container |

### Incremental Execution

RepX tracks job completion state. Interrupted executions resume from the last completed job. Container images are synchronized incrementally to minimize transfer overhead during iterative development.
