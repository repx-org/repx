<div align="center">
  <img src="docs/static/img/logo.svg" height="150" alt="RepX Logo" />
  <h1>RepX</h1>

[Documentation][docs] | [Getting Started][quickstart] | [Installation][install]
</div>

RepX is a framework for defining, executing, and analyzing computational experiments on HPC infrastructure. It uses [Nix](https://nixos.org) to build fully self-contained experiment artifacts that capture every software dependency, script, and parameter. The same artifact runs identically on a laptop, over SSH, or on a SLURM cluster.

[docs]: https://repx-org.github.io/
[quickstart]: ./docs/docs/getting-started/quickstart.md
[install]: ./docs/docs/getting-started/installation.md

## Why RepX?

Experiment frameworks like Snakemake and Nextflow manage workflow execution but leave environment reproducibility to the user (Conda, Docker, manual installs). When the environment breaks, the experiment breaks. RepX takes a different approach: the experiment definition *is* the environment. Nix locks every dependency at build time, and the resulting artifact is portable and hermetic.

- **Environment is part of the build.** Software dependencies are resolved and locked by Nix. If it builds, it runs.

- **Errors surface at build time.** Stage scripts are statically analyzed during `nix build`. Missing commands, bad shell syntax, and undeclared dependencies fail the build, not a running job.

- **Single artifact, any target.** A built Lab contains executables, container images, and host tools. Copy it to any Linux machine and run it -- no Nix required on the target.

- **Parameter sweeps as a DAG.** Parameters are declared as lists. RepX computes the Cartesian product and encodes it into the job dependency graph. Changing one parameter rebuilds only affected nodes.

- **Incremental execution.** Completed jobs persist across runs. Failures don't discard progress.

## Architecture

RepX has three layers:

| Layer | Component | Language | Purpose |
|-------|-----------|----------|---------|
| Definition | [`nix/lib`](./nix/lib) | Nix | Stages, pipelines, parameters, dependency resolution |
| Execution | [`crates/`](./crates) | Rust | DAG scheduling, SSH/SLURM submission, container orchestration, TUI |
| Analysis | [`python/`](./python) | Python | Result querying, parameter tracing, Pandas integration |

## Quick Start

RepX is used as a flake input in your project. See the [examples/](./examples) directory for complete working projects, or follow the [quickstart guide](./docs/docs/getting-started/quickstart.md) to build and run your first experiment.

## Documentation

- [Installation](./docs/docs/getting-started/installation.md)
- [Core Concepts](./docs/docs/getting-started/concepts.md)
- [User Guide](./docs/docs/user-guide/defining-experiments.md)
- [CLI Reference](./docs/docs/reference/cli-reference.md)
- [Nix Functions Reference](./docs/docs/reference/nix-functions.md)
- [Python API Reference](./docs/docs/reference/python-api.md)

## License

RepX is distributed under the MIT License. See [LICENSE](./LICENSE) for details.
