# CLI Reference

The `repx` command-line interface provides experiment management, execution, and debugging capabilities.

## Synopsis

```
repx [OPTIONS] <COMMAND> [ARGS]
```

## Global Options

| Option | Short | Description |
|--------|-------|-------------|
| `--lab <PATH>` | | Lab directory path (default: `./result`) |
| `--verbose` | `-v` | Increase log verbosity (repeatable) |
| `--resources <PATH>` | | Resource configuration file |
| `--target <NAME>` | | Execution target from `config.toml` |
| `--scheduler <TYPE>` | | Override scheduler (`local`, `slurm`) |

## Commands

### repx run

Submit an experiment run for execution.

```
repx run <RUN_NAME> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--dry-run` | Validate submission without execution |
| `--force` | Re-execute completed jobs |
| `--jobs <N>` | Maximum parallel jobs |
| `--native` | Force native execution mode |
| `--bwrap` | Force bubblewrap execution mode |
| `--docker` | Force Docker execution mode |
| `--podman` | Force Podman execution mode |

**Exit Codes:**

| Code | Meaning |
|------|---------|
| 0 | All jobs completed successfully |
| 1 | One or more jobs failed |
| 2 | Configuration or validation error |

### repx list

Display available runs in the Lab.

```
repx list [OPTIONS]
```

Output includes run names and job counts.

### repx tui

Launch the terminal user interface for job monitoring.

```
repx tui [OPTIONS]
```

The TUI provides real-time job status, log streaming, and resource visualization.

### repx viz

Generate experiment topology visualization.

```
repx viz [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--output <PATH>` | Output file (default: `topology.png`) |
| `--format <FMT>` | Output format: `svg`, `png`, `pdf`, `dot` |

### repx debug-run

Execute a job in debug mode with optional interactive shell.

```
repx debug-run <JOB_ID> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--command <CMD>` | Command to execute (default: interactive shell) |

The job environment is fully initialized, enabling inspection of inputs, environment variables, and dependency closures.

### repx trace-params

Display effective parameter values for a job.

```
repx trace-params <JOB_ID>
```

Output shows parameter sources (defaults, overrides) and final resolved values.

### repx gc

Remove stale artifacts from the output store.

```
repx gc [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--dry-run` | Report artifacts to remove without deletion |
| `--keep <N>` | Retain N most recent runs |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `REPX_LOG_LEVEL` | Log verbosity: `error`, `warn`, `info`, `debug`, `trace` |
| `XDG_CONFIG_HOME` | Configuration directory base (default: `~/.config`) |
| `XDG_CACHE_HOME` | Cache directory base (default: `~/.cache`) |

## Configuration Files

| File | Location | Purpose |
|------|----------|---------|
| `config.toml` | `$XDG_CONFIG_HOME/repx/` | Target and global settings |
| `resources.toml` | Working directory or config | SLURM resource mappings |

## Examples

Execute a run locally with limited concurrency:

```bash
repx run simulation --lab ./result -j 4
```

Submit to a remote SLURM cluster:

```bash
repx run training --target cluster --resources gpu-config.toml
```

Debug a failed job:

```bash
repx debug-run abc123-stage-preprocess
```

Generate SVG topology diagram:

```bash
repx viz --format svg --output experiment.svg
```
