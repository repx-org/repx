# Configuration

RepX configuration is managed via `config.toml`, typically located at `~/.config/repx/config.toml` (or `$XDG_CONFIG_HOME/repx/config.toml`).

## Target Configuration

You can define multiple execution targets (e.g., local machine, HPC cluster).

```toml
# Default target if none specified
submission_target = "local"

[targets.local]
base_path = "/home/user/.repx/store"
default_scheduler = "local"

[targets.cluster]
address = "user@hpc-login-node"
base_path = "/scratch/user/repx-store"
default_scheduler = "slurm"

# Optional: Fast local storage for container caching (e.g., NVMe scratch)
node_local_path = "/tmp/user/repx"

# Optional: Impure Host Mounts
# Use this to mount host paths (like /home or /nix/store) into the container.
# Useful for debugging or accessing large datasets not managed by RepX.
mount_host_paths = false
# OR specify specific paths:
# mount_paths = ["/home/user/data", "/opt/tools"]

[targets.cluster.slurm]
# Preference for container runtimes
execution_types = ["podman", "native"]
```

## Resources Configuration (`resources.toml`)

The `resources.toml` file maps jobs to scheduler resources (SLURM partitions, memory, time). RepX evaluates rules in order; the last matching rule applies.

**Location Priority:**
1.  CLI Flag: `--resources <PATH>`
2.  Local file: `./resources.toml`
3.  Global config: `~/.config/repx/resources.toml`

### Schema

```toml
# Global defaults
[defaults]
partition = "main"
cpus-per-task = 1
mem = "4G"
time = "01:00:00"
sbatch_opts = []  # Extra arguments for sbatch

# Specific Rules
[[rules]]
# Glob pattern to match Job IDs
job_id_glob = "*-heavy-*"

# Optional: Only apply this rule for a specific target
target = "cluster"

# Resources to apply
mem = "128G"
cpus-per-task = 16

# Scatter-Gather Specifics
# You can override resources specifically for the 'worker' jobs 
# of a scatter-gather stage.
[rules.worker_resources]
mem = "64G"
cpus-per-task = 4
```
