# Building Labs

The **Lab** is the central artifact in RepX. It is a directory structure produced by Nix that contains everything needed to execute your experiment.

## The Build Command

To build the lab, run:

```bash
nix build
```

This command looks for the `default` package output in your `flake.nix`.

## What's Inside the Lab?

The output is typically a symlink named `result`.

```
result/
├── lab/
│   └── lab-metadata.json   # Lab ID and root metadata reference
├── revision/
│   ├── metadata.json       # DAG structure, run definitions, groups
│   └── <run-hash>.json     # Per-run job manifests
├── jobs/
│   └── <job-hash>/
│       ├── bin/             # Executable scripts
│       └── out/             # Expected output structure
├── host-tools/
│   └── <hash>/
│       └── bin/             # Static binaries for bootstrapping (bash, rsync, jq, etc.)
├── images/                  # Container images (when applicable)
│   └── <image-hash>/
├── store/                   # Nix store closure (software dependencies)
└── readme/                  # Human-readable lab summary
```

### Key Directories

- **`lab/`**: Contains `lab-metadata.json` which points to the root metadata file. This is the entry point the CLI uses to discover the experiment graph.
- **`revision/`**: Contains the root metadata (DAG structure, run definitions, group mappings) and per-run job manifests with all job-level metadata.
- **`jobs/`**: One subdirectory per unique job, containing the executable scripts and output structure templates.
- **`host-tools/`**: Statically-linked binaries (coreutils, bash, jq, rsync, bubblewrap, etc.) for bootstrapping on machines without Nix.
- **`images/`**: Docker/OCI container images when `containerized = true` (the default) in the run definition.
- **`store/`**: The Nix store closure containing all software dependencies.

## Reproducibility Guarantees

Because the Lab is built by Nix:
1.  **Software Environment:** Exact versions of all tools (Python, compilers, libraries) are locked.
2.  **Scripts:** Your stage scripts are part of the build. They undergo [build-time validation](../reference/nix-functions.md#build-time-script-validation) (ShellCheck + dependency analysis).
3.  **Parameters:** All parameters are baked into the job definitions.
4.  **Provenance:** The git commit hash and lab version are recorded in the metadata.

If you copy this `result` directory to another machine (that has Nix or the `host-tools` compatible with the target), you can run the exact same experiment.

## Inspecting the Lab

After building, you can inspect the lab contents:

```bash
# List all runs
repx list runs

# List all jobs with parameters
repx list jobs -p seed -p model

# Visualize the experiment topology
repx viz --format svg --output topology.svg

# Show detailed job information
repx show job <JOB_ID>
```
