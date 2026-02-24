# CLI Reference

The `repx` command-line interface provides experiment management, execution, visualization, and debugging capabilities.

## Synopsis

```
repx [OPTIONS] <COMMAND> [ARGS]
```

Use `repx --help-all` to print help for all commands and subcommands recursively.

## Global Options

| Option | Short | Description |
|--------|-------|-------------|
| `--lab <PATH>` | | Lab directory path (default: `./result`) |
| `--verbose` | `-v` | Increase log verbosity (repeatable: `-v`, `-vv`, `-vvv`) |
| `--resources <PATH>` | | Resource configuration file path |
| `--target <NAME>` | | Execution target from `config.toml` |
| `--scheduler <TYPE>` | | Override scheduler: `local`, `slurm` |
| `--help-all` | | Print help for all commands recursively |

---

## Commands

### repx run

Execute one or more runs or individual jobs.

```
repx run [RUN_OR_JOB_ID...] [OPTIONS]
```

Accepts **multiple** run names or job IDs. If none are specified, all runs in the Lab are executed.

| Option | Short | Description |
|--------|-------|-------------|
| `--jobs <N>` | `-j` | Maximum parallel jobs |
| `--continue-on-failure` | | Continue executing independent jobs when some fail. All failures are reported at the end. |

**Exit Codes:**

| Code | Meaning |
|------|---------|
| 0 | All jobs completed successfully |
| 1 | One or more jobs failed |
| 2 | Configuration or validation error |

**Examples:**

```bash
# Execute a single run
repx run simulation --lab ./result -j 4

# Execute multiple runs
repx run training validation

# Execute a specific job by ID
repx run abc123def456

# Continue despite failures
repx run simulation --continue-on-failure
```

### repx list

Inspect runs, jobs, dependencies, and groups in the Lab.

```
repx list <ENTITY> [ARGS] [OPTIONS]
```

#### repx list runs

List all runs defined in the Lab, or show details for a specific run.

```
repx list runs [RUN_NAME]
```

#### repx list jobs

List all jobs, optionally filtered by run or stage.

```
repx list jobs [RUN_NAME] [OPTIONS]
```

| Option | Short | Description |
|--------|-------|-------------|
| `--stage <NAME>` | `-s` | Filter by stage name (substring match) |
| `--output-paths` | | Show output directory paths |
| `--param <KEY>` | `-p` | Show effective parameter values (repeatable for multiple keys) |
| `--group-by-stage` | `-g` | Group output by stage name |

**Examples:**

```bash
# List all jobs
repx list jobs

# List jobs in a specific run
repx list jobs simulation

# Filter by stage and show parameters
repx list jobs simulation -s train -p seed -p learning_rate

# Show output paths grouped by stage
repx list jobs --output-paths --group-by-stage
```

#### repx list deps

Show dependencies for a specific job (alias: `dependencies`).

```
repx list deps <JOB_ID>
```

#### repx list groups

List all run groups, or show runs in a specific group.

```
repx list groups [GROUP_NAME]
```

### repx show

Inspect detailed job information and output artifacts.

```
repx show <ENTITY> <JOB_ID> [ARGS]
```

#### repx show job

Display comprehensive information about a job: name, run, status, stage type, parameters, resource hints, inputs, outputs, file paths, log locations, and output file listing with sizes.

```
repx show job <JOB_ID>
```

#### repx show output

View the contents of a job's output files. Without a path argument, lists all output files. With a path, displays the file contents or directory listing.

```
repx show output <JOB_ID> [PATH]
```

**Examples:**

```bash
# Show job details
repx show job abc123def456

# List all output files for a job
repx show output abc123def456

# Display a specific output file
repx show output abc123def456 results.csv
```

### repx tui

Launch the terminal user interface for interactive job monitoring.

```
repx tui [OPTIONS]
```

The TUI provides real-time job status, log streaming, resource visualization, and interactive job management. See [TUI Reference](../running-experiments/tui.md) for keybinding details.

### repx viz

Generate experiment topology visualization. Requires [Graphviz](https://graphviz.org/) to be installed.

```
repx viz [OPTIONS]
```

| Option | Short | Description |
|--------|-------|-------------|
| `--output <PATH>` | `-o` | Output file path (default: `topology`) |
| `--format <FMT>` | | Output format: `svg`, `png`, `pdf`, `dot` |

### repx debug-run

Execute a job in debug mode with an optional interactive shell. The job environment is fully initialized, enabling inspection of inputs, environment variables, and dependency closures.

```
repx debug-run <JOB_ID> [OPTIONS]
```

| Option | Short | Description |
|--------|-------|-------------|
| `--command <CMD>` | `-c` | Command to execute (default: interactive shell) |

### repx trace-params

Display effective parameter values and their sources for a job, tracing inheritance through the dependency graph.

```
repx trace-params [JOB_ID]
```

### repx gc

Remove stale artifacts from the output store.

```
repx gc [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--target <NAME>` | Target to garbage collect |

### repx completions

Generate shell completion scripts.

```
repx completions --shell <SHELL>
```

Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

**Example:**

```bash
# Generate bash completions
repx completions --shell bash > ~/.local/share/bash-completion/completions/repx

# Generate zsh completions
repx completions --shell zsh > ~/.zfunc/_repx
```

---

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

Inspect job details:

```bash
repx show job abc123-stage-preprocess
```

List jobs with specific parameters:

```bash
repx list jobs simulation -s train -p seed -p model --group-by-stage
```

Generate SVG topology diagram:

```bash
repx viz --format svg --output experiment.svg
```
