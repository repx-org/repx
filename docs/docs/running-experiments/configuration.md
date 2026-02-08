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

## Resources Configuration

The `resources.toml` file maps jobs to scheduler resources. Rules are evaluated sequentially; the last matching rule takes precedence.

### Resolution Order

1. CLI flag: `--resources <PATH>`
2. Working directory: `./resources.toml`
3. Global config: `~/.config/repx/resources.toml`

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
target = "cluster"          # Optional: target-specific rule
mem = "128G"
cpus-per-task = 16

# Scatter-gather worker overrides
[rules.worker_resources]
mem = "64G"
cpus-per-task = 4
```

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

## Stage Types

Jobs are classified by their execution semantics:

| Type | Description |
|------|-------------|
| `simple` | Standard single-execution job |
| `scatter-gather` | Parallel fan-out orchestration |
| `worker` | Individual scatter partition |
| `gather` | Aggregation of worker outputs |

## Logging Configuration

Log output is controlled via environment variables and CLI flags:

| Method | Example |
|--------|---------|
| Environment | `REPX_LOG_LEVEL=debug repx run ...` |
| CLI flag | `repx -vv run ...` |

Valid log levels: `error`, `warn`, `info`, `debug`, `trace`
