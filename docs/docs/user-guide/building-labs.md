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
│   ├── manifest.json       # Complete experiment topology
│   └── runs.json           # List of defined runs
├── jobs/
│   └── <job_hash>/         # One directory per unique job
│       ├── script          # The executable script
│       ├── env             # The software environment (closure)
│       └── metadata.json   # Job-specific metadata
├── host-tools/             # Static binaries for bootstrapping
└── revision/               # Git commit hash and dirty status
```

## Reproducibility Guarantees

Because the Lab is built by Nix:
1.  **Software Environment:** Exact versions of all tools (Python, compilers, libraries) are locked.
2.  **Scripts:** Your stage scripts are part of the build.
3.  **Parameters:** All parameters are baked into the job definitions.

If you copy this `result` directory to another machine (that has Nix or the `host-tools` compatible with the target), you can run the exact same experiment.
