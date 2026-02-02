# CLI Reference

The `repx` CLI is the primary tool for managing your experiments.

## Global Options

These options apply to all commands.

*   `--lab <path>`: Path to the built lab directory (default: `./result`).
*   `--verbose` / `-v`: Increase verbosity level.
*   `--resources <path>`: Path to a `resources.toml` file (for execution resource requirements).
*   `--target <name>`: The execution target (defined in `config.toml`) to use.
*   `--scheduler <type>`: Override the scheduler (`slurm` or `local`).

## Commands

### `repx run`

Submits an experiment run to the execution target.

**Usage:**
```bash
repx run <RUN_NAME> [OPTIONS]
```

**Options:**
*   `--dry-run`: Simulate the submission without executing jobs.

### `repx tui`

Opens the Terminal User Interface dashboard to monitor jobs.

**Usage:**
```bash
repx tui [OPTIONS]
```

### `repx viz`

Visualizes the experiment topology as a graph.

**Usage:**
```bash
repx viz [OPTIONS]
```

**Options:**
*   `--output <path>`: Output file path (default: `topology.png`).
*   `--format <fmt>`: Output format (e.g., `svg`, `png`, `pdf`, `dot`).

### `repx debug-run`

Debugs a specific job by running it locally, optionally dropping into a shell.

**Usage:**
```bash
repx debug-run <JOB_ID> [OPTIONS]
```

**Options:**
*   `--command <cmd>`: The command to run inside the job environment (default: interactive shell).

### `repx trace-params`

Traces and displays the effective parameters for a specific job, including sources (defaults vs overrides).

**Usage:**
```bash
repx trace-params <JOB_ID>
```

### `repx gc`

Garbage Collects old run artifacts to free up space.

**Usage:**
```bash
repx gc
```

### `repx list`

Lists available runs in the current Lab.

**Usage:**
```bash
repx list
```
