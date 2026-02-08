# Remote Execution

RepX provides transparent execution on remote systems via SSH, with optional SLURM workload manager integration. The Lab artifact and its dependencies are synchronized to the target before execution.

## Prerequisites

| Requirement | Description |
|-------------|-------------|
| SSH access | Passwordless key-based authentication to target host |
| Storage | Writable directory on target for artifacts and outputs |
| Configuration | Target definition in `config.toml` |

## Target Configuration

Define remote targets in `~/.config/repx/config.toml`:

```toml
[targets.cluster]
address = "user@login.cluster.edu"
base_path = "/scratch/user/repx_work"
default_scheduler = "slurm"
node_local_path = "/tmp/user/repx"
```

### Configuration Parameters

| Parameter | Required | Description |
|-----------|----------|-------------|
| `address` | yes | SSH connection string |
| `base_path` | yes | Remote working directory |
| `default_scheduler` | yes | `local` or `slurm` |
| `node_local_path` | no | Node-local storage for container caching |

## Execution

Specify the target via the `--target` flag:

```bash
repx run simulation --lab ./result --target cluster
```

## Resource Allocation (SLURM)

SLURM resource requirements are specified in `resources.toml`:

```toml
[defaults]
time = "01:00:00"
mem = "4G"
cpus-per-task = 1
partition = "standard"

[[rules]]
job_id_glob = "*training*"
time = "12:00:00"
mem = "64G"
sbatch_opts = ["--gres=gpu:1"]

[[rules]]
job_id_glob = "*preprocess*"
mem = "32G"
partition = "highmem"
```

Apply resources during execution:

```bash
repx run simulation --lab ./result --target cluster --resources resources.toml
```

## Synchronization

RepX employs a multi-phase synchronization strategy:

### Phase 1: Bootstrap

Static host tools (bash, jq, rsync, etc.) are deployed to ensure consistent execution regardless of target system configuration.

### Phase 2: Lab Transfer

The Lab artifact is synchronized using `rsync` with the following characteristics:

- Incremental transfer of changed files only
- Preservation of symbolic links and permissions
- Atomic updates via temporary staging

### Phase 3: Container Image Sync (Incremental)

Container images are synchronized incrementally to minimize transfer overhead:

1. Image manifest is compared against remote cache
2. Only modified or missing layers are transferred
3. Existing layers are reused from `node_local_path` when available

This optimization significantly reduces synchronization time for iterative development workflows where container images change infrequently.

### Phase 4: Job Submission

| Scheduler | Mechanism |
|-----------|-----------|
| SLURM | Generates `sbatch` scripts and submits via `sbatch` |
| Local | Spawns RepX agent process for direct execution |

## Directory Structure

Remote artifacts are organized under `base_path`:

```
<base_path>/
  artifacts/          # Lab closure and job packages
  outputs/            # Job execution outputs
    <job-id>/
      out/            # User artifacts
      repx/           # Execution metadata
  host-tools/         # Static tool binaries
    <hash>/bin/
  bin/                # Deployed utilities
```

## Troubleshooting

### Connection Issues

Verify SSH connectivity:

```bash
ssh -o BatchMode=yes user@host echo "Connection successful"
```

### Permission Errors

Ensure `base_path` is writable:

```bash
ssh user@host "mkdir -p /scratch/user/repx_work && touch /scratch/user/repx_work/.test"
```

### SLURM Submission Failures

Check SLURM queue status and partition availability:

```bash
ssh user@host "sinfo -p partition_name"
```
