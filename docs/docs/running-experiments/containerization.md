# Containerization

RepX isolates job execution using container technologies to ensure reproducibility. Host system libraries are excluded from the execution environment; only dependencies specified in the Nix closure are available.

## Runtime Backends

RepX supports multiple container runtimes:

| Runtime | Description | Privileges Required |
|---------|-------------|---------------------|
| `native` | Direct process execution (no isolation) | None |
| `bwrap` | Bubblewrap namespace isolation | None (user namespaces) |
| `docker` | Docker container engine | Root or docker group |
| `podman` | Podman rootless containers | None |

## Isolation Properties

The container environment enforces the following constraints:

| Property | Configuration |
|----------|---------------|
| Root filesystem | Read-only |
| Nix store | Mounted read-only at `/nix/store` |
| Output directory | Mounted read-write at `$out` |
| Input artifacts | Bind-mounted from upstream jobs |
| Network | Disabled by default |

## Configuration

### Per-Target Runtime Selection

Specify runtime preference in target configuration:

```toml
[targets.local.local]
execution_types = ["bwrap", "native"]

[targets.cluster.slurm]
execution_types = ["podman", "native"]
```

The first available runtime in the list is selected.

### CLI Override

Force a specific runtime via command-line flags:

```bash
repx run simulation --bwrap
repx run simulation --docker
repx run simulation --podman
repx run simulation --native
```

## Bubblewrap Execution

Bubblewrap (`bwrap`) provides lightweight namespace isolation without requiring elevated privileges. It is the recommended runtime for HPC environments.

### Rootfs Extraction

Container images are extracted to a rootfs directory for `bwrap` execution:

1. Image tarball is located in the Lab's `image/` directory
2. Extraction occurs to `node_local_path` if configured, otherwise `base_path`
3. Extracted rootfs is cached by image hash for reuse

### Mount Configuration

Default `bwrap` mounts:

| Path | Type | Purpose |
|------|------|---------|
| `/nix/store` | ro-bind | Nix closure access |
| `/tmp` | tmpfs | Temporary storage |
| `$out` | bind | Output directory |
| Job inputs | ro-bind | Upstream artifacts |

## Impure Mode

For debugging or accessing host resources, impure mode relaxes isolation:

```toml
[targets.local]
mount_host_paths = true
# Or specify explicit paths:
mount_paths = ["/home/user/data", "/opt/tools"]
```

Impure mode compromises reproducibility and should be used only for development.

## Debugging

Inspect the container environment with `repx debug-run`:

```bash
repx debug-run <job_id> --lab ./result
```

This spawns an interactive shell within the job's execution environment, with all mounts and environment variables configured identically to normal execution.
