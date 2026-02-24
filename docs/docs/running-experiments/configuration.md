# Configuration

RepX uses TOML configuration files following XDG Base Directory conventions. The primary configuration file is located at `~/.config/repx/config.toml` (or `$XDG_CONFIG_HOME/repx/config.toml`).

## Target Configuration

Targets define execution environments. Each target specifies connection parameters, storage paths, and scheduler preferences.

```toml
# Default target when --target is not specified
submission_target = "local"

[targets.local]
base_path = "/home/user/.repx/store"
default_scheduler = "local"

[targets.cluster]
address = "user@hpc-login-node"
base_path = "/scratch/user/repx-store"
default_scheduler = "slurm"

# Node-local storage for container image caching
# Recommended for NVMe scratch or local SSD paths
node_local_path = "/tmp/user/repx"

# Host path mounting (impure mode)
mount_host_paths = false
# mount_paths = ["/home/user/data", "/opt/tools"]

[targets.cluster.slurm]
execution_types = ["podman", "native"]
```

### Target Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `address` | string | SSH connection string (`user@host`) |
| `base_path` | path | Root directory for artifacts and outputs |
| `default_scheduler` | enum | `local` or `slurm` |
| `node_local_path` | path | Fast local storage for container caching |
| `mount_host_paths` | bool | Enable impure host path mounting |
| `mount_paths` | array | Explicit paths to mount into containers |

### Scheduler Types

RepX supports two scheduler backends:

| Scheduler | Description |
|-----------|-------------|
| `local` | Direct process execution with configurable concurrency |
| `slurm` | SLURM workload manager integration via `sbatch` |

## Resource System

RepX has a three-tier resource system that lets scientists declare expected resource requirements in Nix while allowing cluster admins to override them per-cluster via TOML configuration.

### How Resources Flow

Resources can be defined in two places:

1. **In Nix stage files** -- baked into the Lab metadata at build time
2. **In `resources.toml`** -- applied at runtime, per-cluster

At runtime, these are merged with a clear priority order:

```
resources.toml [defaults]     (lowest priority)
        ↓
Nix resource_hints             (overrides defaults)
        ↓
resources.toml [[rules]]      (highest priority -- overrides Nix hints)
```

This means:
- `resources.toml` **defaults** provide a baseline for all jobs
- **Nix hints** override defaults with per-stage requirements defined by the experiment author
- `resources.toml` **rules** (matched by glob pattern) override everything, letting the cluster admin tune resources for specific jobs or hardware

### Nix-Defined Resources

Resources are declared in stage `.nix` files using the `resources` attribute. See the [Nix Functions Reference](../reference/nix-functions.md#resource-hints) for full details.

**Static resources:**
```nix
resources = {
  mem = "256M";
  cpus = 1;
  time = "00:02:00";
};
```

**Dynamic resources** (varying per parameter combination):
```nix
resources = { params }: {
  mem = if params.mode == "slow" then "4G" else "1G";
  cpus = if params.multiplier > 5 then 4 else 1;
};
```

**Scatter-gather sub-stage and step resources:**
```nix
resources = { mem = "256M"; cpus = 1; };  # orchestrator-level

# Steps can each declare their own resources
steps = {
  compute = {
    resources = { mem = "2G"; cpus = 2; time = "00:30:00"; };
    # ...
  };
  analyze = {
    resources = { mem = "4G"; cpus = 4; time = "01:00:00"; };
    deps = [ compute ];
    # ...
  };
};

gather = {
  resources = { mem = "1G"; cpus = 1; time = "00:10:00"; };
  # ...
};
```

#### Propagation Through Dependencies

When a stage depends on upstream stages, `callStage` automatically collects resource hints from all upstream `passthru.resources` and merges them using **max semantics** -- the largest `mem`, `cpus`, and `time` across all inputs is used as the baseline. The stage's own `resources` then override on top.

This means a heavy upstream stage's resource profile propagates to downstream stages unless the downstream stage declares larger requirements.

#### Resources Are Metadata-Only

Changing resource hints does **not** affect Nix derivation hashes. You can adjust resources without triggering a rebuild of the Lab.

### `resources.toml` Configuration

The `resources.toml` file provides runtime resource overrides. Multiple files are deep-merged in this order:

1. Global config: `~/.config/repx/resources.toml`
2. Working directory: `./resources.toml`
3. CLI flag: `--resources <PATH>`

Later sources override earlier ones.

### Schema

```toml
[defaults]
partition = "main"
cpus-per-task = 1
mem = "4G"
time = "01:00:00"
sbatch_opts = []

[[rules]]
job_id_glob = "*-heavy-*"
target = "cluster"          # Optional: only apply on this target
mem = "128G"
cpus-per-task = 16

[[rules]]
job_id_glob = "*-scatter*"
mem = "500M"

# Override resources specifically for scatter-gather steps
[rules.step_resources]
mem = "16G"
cpus-per-task = 4
```

Rules are evaluated in order. The **last matching rule** takes precedence.

### Resource Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `partition` | string | SLURM partition name |
| `cpus-per-task` | int | CPU cores per job |
| `mem` | string | Memory limit (e.g., `4G`, `512M`) |
| `time` | string | Wall time limit (`HH:MM:SS`) |
| `sbatch_opts` | array | Additional `sbatch` arguments |
| `job_id_glob` | pattern | Glob pattern for job ID matching |
| `target` | string | Restrict rule to specific target |
| `step_resources` | table | Nested resource overrides for scatter-gather steps |

### Step Resource Resolution

For scatter-gather stages, step resources are resolved separately per step:

1. Start with the **orchestrator's resolved resources** (the three-tier merge above)
2. Apply **Nix step `resource_hints`** (from the step's `resources` attribute in the stage definition)
3. Apply **`resources.toml` `[rules.step_resources]`** (if a matching rule has this nested table)

If no step-specific overrides exist, steps inherit the orchestrator's resources. Each step in the DAG can have its own resource requirements, allowing fine-grained control (e.g., a trace generation step needing 32G while a lightweight analysis step needs only 1G).

### Effect on Local Execution

Even without SLURM, resources affect local execution. The local scheduler uses resolved `mem` and `cpus` values for **admission control** -- it tracks total available RAM and CPUs on the machine and prevents over-subscription by queuing jobs that don't fit.

### Inspecting Resources

Use `repx show job <JOB_ID>` to see the Nix-defined resource hints for any job:

```bash
repx show job abc123def456
# Shows "Resource Hints (from Nix)" section with mem, cpus, time, partition
# For scatter-gather stages, shows per-sub-stage resource hints
```

## Stage Types

Jobs are classified by their execution semantics:

| Type | Description |
|------|-------------|
| `simple` | Standard single-execution job |
| `scatter-gather` | Parallel fan-out orchestration |
| `step` | Individual step within a scatter-gather branch DAG |
| `gather` | Aggregation of step outputs |

## Logging Configuration

Log output is controlled via environment variables and CLI flags:

| Method | Example |
|--------|---------|
| Environment | `REPX_LOG_LEVEL=debug repx run ...` |
| CLI flag | `repx -vv run ...` |

Valid log levels: `error`, `warn`, `info`, `debug`, `trace`
