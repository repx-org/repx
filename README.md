# repx-rs

**repx-rs** is the execution engine for the RepX framework. Written in Rust, it provides the command-line interface (CLI) and Terminal User Interface (TUI) for submitting, orchestrating, and monitoring experiments defined via `repx-nix`.

## Overview

The `repx-runner` binary handles the complexity of executing jobs across different environments. It supports local execution for development and remote execution via SSH for production HPC clusters. It abstracts away scheduler interaction (SLURM) and containerization (Bubblewrap, Docker, Podman).

## Components

This workspace contains the following crates:

*   **repx-runner:** The primary CLI binary for executing jobs and managing the lab.
*   **repx-tui:** A terminal-based dashboard for real-time monitoring and management.
*   **repx-client:** The core logic for communicating with execution targets.
*   **repx-executor:** Handles the low-level process execution and container runtime wrapping.
*   **repx-core:** Shared configuration and data models.

## Installation

### Prerequisites
*   Rust (latest stable)
*   `pkg-config`
*   OpenSSL development headers

### Build
```bash
cargo build --release
```
The binaries will be located at `target/release/repx-runner` and `target/release/repx-tui`.

## Configuration

Configuration is managed via `config.toml`, typically located at `~/.config/repx/config.toml` (or `$XDG_CONFIG_HOME/repx/config.toml`).

```toml
submission_target = "cluster"

[targets.local]
base_path = "/home/user/repx-store"
default_scheduler = "local"

[targets.cluster]
address = "user@hpc-login-node"
base_path = "/scratch/user/repx-store"
default_scheduler = "slurm"
# Optional: Fast local storage for container caching
node_local_path = "/tmp/user/repx" 

[targets.cluster.slurm]
execution_types = ["podman", "native"]
```

## Resources Configuration

You can define resource requirements (SLURM partition, walltime, memory) via a `resources.toml` file. `repx` applies these rules by matching against job IDs.

Location priority:
1.  Correctly specified via CLI: `--resources <PATH>`
2.  Local file: `./resources.toml`
3.  Global config: `~/.config/repx/resources.toml`

**Example `resources.toml`:**

```toml
# Default settings for all jobs
[defaults]
partition = "main"
cpus-per-task = 1
mem = "4G"
time = "01:00:00"

# Specific overrides based on job ID patterns
[[rules]]
job_id_glob = "*-heavy-*"
partition = "high-mem"
mem = "64G"
```

## Supported Runtimes

`repx-runner` supports multiple execution runtimes, which can be configured per target or scheduler.

*   **`native`**: Runs directly on the host. Simplest, but requires all dependencies to be present.
*   **`bwrap`**: Uses Bubblewrap for lightweight sandboxing. High performance, ideal for local usage and HPC where unprivileged user namespaces are allowed.
*   **`podman`**: Uses Podman for OCI container execution. Recommended for Slurm clusters.
*   **`docker`**: Uses Docker. Typically for local development or rigid enterprise environments.

## `repx-runner` CLI Usage

The `repx-runner` is the workhorse of the framework. It handles job submission, orchestration, and garbage collection.

### Global Flags
These flags apply to all commands:
*   `--lab <PATH>`: Path to the lab result directory (default: `./result`).
*   `--resources <PATH>`: Path to a `resources.toml` file defining execution requirements.
*   `--target <NAME>`: The execution target to use (must be defined in `config.toml`). Overrides `submission_target` in config.
*   `--scheduler <NAME>`: Scheduler to use (`slurm` or `local`). Overrides the target's default.
*   `-v` / `-vv`: Increase verbosity for debugging.

### Commands

#### `run`
Submit a run or specific jobs.

```bash
# Run a specific named run defined in the lab
repx-runner run simulation-run --target local

# Run specific jobs by ID
repx-runner run <JOB_ID_1> <JOB_ID_2>

# Limit parallelism (local scheduler only)
repx-runner run simulation-run --jobs 4
```

#### `list`
List available runs and jobs in the lab.

```bash
repx-runner list

repx-runner list jobs simulation-run

repx-runner list deps <JOB_ID>
```

#### `gc`
Garbage collect unused artifacts and outputs from the target.

```bash
repx-runner gc --target cluster
```

### CI/CD Integration

`repx-runner` is designed for use in CI/CD pipelines. It returns a non-zero exit code if any job fails.

**Common Pattern (Remote SLURM):**
Automate job submission to an HPC cluster from a CI runner.

1.  **Configure environment:** Define `config.toml` and (optionally) `resources.toml` in the CI job.
2.  **Authenticate:** Ensure SSH access to the cluster (e.g., using `ssh-agent`).
3.  **Run:**

```bash
# Example CI Script
export RUST_BACKTRACE=1

# Create a temporary config for the CI run
cat <<EOF > ci-config.toml
submission_target = "cluster"
[targets.cluster]
address = "user@hpc-login-node"
base_path = "/scratch/ci-builds/${CI_JOB_ID}"
default_scheduler = "slurm"
default_execution_type = "podman"
[targets.cluster.slurm]
execution_types = ["podman"]
EOF

# Submit jobs and wait for completion
repx-runner run validation-suite \
  --lab ./result \
  --target cluster \
  --scheduler slurm \
  -vv
```

## Debugging & Logs for LLMs

While the TUI provides an interactive way to inspect logs, automated scripts can access the output files directly. The directory structure corresponds to the `base_path` defined in your target configuration.

**Directory Structure:**

```text
<base_path>/                     # Root output directory
└── outputs/
    └── <JOB_ID>/                # Directory for a specific job
        ├── out/                 # User outputs (cwd of the job script)
        │   └── result.json
        └── repx/                # Internal logs and metadata
            ├── stdout.log       # Standard Output of the job execution
            ├── stderr.log       # Standard Error of the job execution
            └── slurm-1234.out   # SLURM output log (if applicable)
```

**Common Debugging Actions:**
*   **Check Script Output:** Read `<base_path>/outputs/<JOB_ID>/repx/stdout.log` or `stderr.log`.
*   **Check Scheduler Output:** If running on Slurm, check `<base_path>/outputs/<JOB_ID>/repx/slurm-*.out`.
*   **Check Job Results:** Look for files in `<base_path>/outputs/<JOB_ID>/out/`.

## `repx-tui` Reference

The `repx-tui` provides an interactive dashboard to monitor jobs, logs, and artifacts.

### Navigation

| Key | Action |
| :--- | :--- |
| `2` | Switch to **Jobs** panel |
| `4` | Switch to **Targets** panel |
| `Space` | Open **Action Menu** (Run, Cancel, Debug, etc.) |
| `g` | Open **Go-To Menu** (Quick navigation) |
| `q` | Quit |

### Jobs Panel
When the jobs panel is focused:

| Key | Action |
| :--- | :--- |
| `j` / `↓` | Next job |
| `k` / `↑` | Previous job |
| `t` | Toggle tree view (hierarchical vs flat) |
| `.` | Toggle collapse/expand of selected tree node |
| `x` | Toggle selection and move down (multiselect) |
| `%` | Select all |
| `/` or `f` | **Filter Mode**: Type to filter jobs by name |
| `l` | Cycle forward through status filters (Pending, Running, Failed, Success) |
| `h` | Cycle backward through status filters |
| `r` | Toggle reverse sort order |

### Targets Panel
When the targets panel is focused:

| Key | Action |
| :--- | :--- |
| `j` / `↓` | Next target |
| `k` / `↑` | Previous target |
| `Enter` | Set selected target as **Active** |

### Menus

**Space Menu (Actions)**
*   `r`: **Run** selected jobs
*   `c`: **Cancel** selected jobs
*   `d`: **Debug** (inspect) selected job
*   `p`: **Path** (show output path)
*   `l`: Show global **Logs**
*   `y`: **Yank** (copy) path to clipboard
*   `e`: **Explore** output directory (opens `yazi` or shell)

**G Menu (Go To)**
*   `g`: Go to top
*   `e`: Go to end
*   `d`: Open job **Definition**
*   `l`: Open job **Logs**

### External Tools
The TUI integrates with external tools for an enhanced experience:
*   **`yazi`**: Used for file exploration when pressing `e` on a job.
*   **`$EDITOR`**: Used for opening files. Defaults to `xdg-open` locally or `vi` remotely.
