# Local Execution

Local execution is the simplest way to run RepX experiments. It uses your local machine's resources to execute the jobs defined in your Lab.

## Basic Usage

To run a specific named run locally:

```bash
repx run <run_name> --lab ./result
```

Example:
```bash
repx run simulation --lab ./result
```

## Concurrency

By default, RepX will try to execute jobs in parallel based on the number of logical cores available on your machine. You can limit this using the `--jobs` or `-j` flag.

```bash
# Limit to 4 parallel jobs
repx run simulation --lab ./result -j 4
```

## Output Handling

*   **Standard Output/Error:** Logs for each job are captured and stored in the job's output directory. They are streamed to the TUI during execution.
*   **Artifacts:** File outputs are generated in the RepX store directory (usually under `~/.repx/storage` or defined by configuration).

<div align="center">
  <img src="/images/simple-tui.png" alt="Execution TUI" />
</div>

## Partial Execution

If an experiment is interrupted, running the command again will resume execution. RepX checks if outputs already exist and skips completed jobs (unless `--force` is used).
