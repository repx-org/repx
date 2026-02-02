# Remote Execution (SSH/SLURM)

RepX is designed to seamlessly move experiments from your laptop to HPC clusters without code changes. It achieves this by synchronizing the "Lab" artifact and its dependencies to the remote host.

## Prerequisites

1.  **SSH Access:** You must have passwordless SSH access (key-based auth) to the target machine.
2.  **Configuration:** You need to define the target in your `config.toml`.

## 1. Configuring Targets

Create or edit `~/.config/repx/config.toml`:

```toml
[targets.my-cluster]
# The SSH address
address = "user@login.cluster.edu"

# The working directory on the remote host (must exist)
base_path = "/scratch/user/repx_work"

# The scheduler to use ("slurm" or "local")
default_scheduler = "slurm"

# Optional: Enable faster local storage for container caching if available
node_local_path = "/tmp/user/repx"
```

## 2. Running on a Target

Specify the target name using the `--target` flag:

```bash
repx run simulation --lab ./result --target my-cluster
```

## 3. Configuring Resources (SLURM)

When using the SLURM scheduler, you need to tell RepX how much memory, time, and CPU/GPU resources each job needs. This is done via a `resources.toml` file.

**resources.toml**:
```toml
# Default settings for all jobs if no specific rule matches
[defaults]
time = "01:00:00"
mem = "4G"
cpus-per-task = 1
partition = "standard"

# Rule: Jobs with "training" in their ID get GPU resources
[[rules]]
job_id_glob = "*training*"
time = "12:00:00"
mem = "64G"
sbatch_opts = ["--gres=gpu:1"]

# Rule: "preprocess" jobs need more memory
[[rules]]
job_id_glob = "*preprocess*"
mem = "32G"
partition = "highmem"
```

Pass this file during execution:

```bash
repx run simulation \
  --lab ./result \
  --target my-cluster \
  --resources resources.toml
```

## How It Works

1.  **Sync**: RepX uses `rsync` (or `nix copy` if available) to transfer the Lab closure to the `base_path` on the remote host.
2.  **Bootstrap**: It installs a static set of tools (the `host-tools` package containing bash, jq, etc.) to ensure a consistent environment even if the cluster has old software.
3.  **Submission**:
    *   **SLURM**: Generates `.sbatch` scripts wrapping your stage scripts and submits them via `sbatch`.
    *   **Local**: Starts a RepX agent on the remote node to execute jobs directly.
