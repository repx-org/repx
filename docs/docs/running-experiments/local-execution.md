# Local Execution

Local execution runs experiments on the host machine using direct process spawning or containerized environments.

## Basic Usage

Execute a named run:

```bash
repx run <run_name> --lab ./result
```

## Concurrency Control

RepX parallelizes job execution based on available CPU cores. Override the default with `--jobs`:

```bash
repx run simulation --lab ./result -j 4
```

## Execution Modes

| Mode | Flag | Description |
|------|------|-------------|
| Native | `--native` | Direct process execution |
| Bwrap | `--bwrap` | Bubblewrap sandboxed execution |
| Docker | `--docker` | Docker container runtime |
| Podman | `--podman` | Podman container runtime |

## Output Structure

Job outputs are organized under the configured `base_path`:

```
<base_path>/
  outputs/
    <job-id>/
      out/              # User artifacts
      repx/
        stdout.log      # Standard output capture
        stderr.log      # Standard error capture
        SUCCESS|FAIL    # Completion marker
```

### Completion Markers

| Marker | Description |
|--------|-------------|
| `SUCCESS` | Job completed with exit code 0 |
| `FAIL` | Job terminated with non-zero exit code |

<div align="center">
  <img src="/images/simple-tui.png" alt="Execution TUI" />
</div>

## Incremental Execution

RepX tracks job completion state. Subsequent invocations skip completed jobs unless `--force` is specified:

```bash
# Resume interrupted execution
repx run simulation --lab ./result

# Force re-execution of all jobs
repx run simulation --lab ./result --force
```

## Runtime Selection

The execution runtime is selected based on Lab configuration and CLI flags:

1. CLI flags take precedence (`--native`, `--bwrap`, etc.)
2. Target configuration specifies `execution_types` preference
3. Lab metadata indicates native-only vs. container-capable

Native-only Labs (no container images) will fail if container execution is requested. Use `--native` explicitly for such Labs.
