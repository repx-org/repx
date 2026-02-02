# Architecture

RepX is a hybrid system combining the build-time guarantees of Nix with a runtime orchestration layer written in Rust.

## Components

### 1. The Definition Layer (`repx` Nix library)
*   **Role:** Defines the experiment graph (DAG).
*   **Output:** A "Lab" derivation.
*   **Mechanism:** Uses Nix to resolve dependencies, fetch software, and generate static build scripts and metadata JSONs.

### 2. The Execution Layer (`repx` CLI)
*   **Role:** Executes the Lab.
*   **Language:** Rust.
*   **Key Crates:**
    *   `repx-cli`: Unified entry point.
    *   `repx-runner`: Core logic for traversing the graph and submitting jobs.
    *   `repx-executor`: Handles the actual execution (process spawning, container management).
    *   `repx-tui`: Terminal interface.

### 3. The Analysis Layer (`repx-py`)
*   **Role:** Consumes results.
*   **Language:** Python.
*   **Mechanism:** Parses the Lab's `manifest.json` and job metadata to provide a queryable interface.

## The "Lab" Artifact

The Lab is the interface between Definition and Execution.

```
/lab
  manifest.json  (The DAG)
/jobs
  <hash>/
    script       (The executable)
    env/         (The closure)
```

The CLI does not need to know *how* the lab was built, only how to read this structure. This allows the CLI to be stable while the Nix DSL evolves.
