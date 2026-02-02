# Advanced Patterns

## Dynamic Stages

In some cases, you might not know the output filenames or the number of outputs at build time. RepX supports dynamic stages where a downstream stage can depend on a directory output from an upstream stage, rather than a specific file.

## Impure Builds

While RepX encourages purity, practical research often requires "impure" access to the host, such as:
*   Large datasets stored on a shared cluster filesystem (Lustre/GPFS).
*   Proprietary license servers.

You can configure "impure" mounts in `config.toml`:

```toml
[targets.cluster]
mount_host_paths = true
```

Or specific paths:

```toml
[targets.cluster]
mount_paths = ["/data/shared/imagenet"]
```

**Note:** Impure builds sacrifice strict reproducibility for convenience. Ensure you document external dependencies.

## Incremental Builds

RepX leverages Nix's caching. If you change a parameter in a downstream stage, only that stage and its dependents will be rebuilt. Upstream stages that haven't changed will be reused from the cache (or simply not re-executed if the output exists).
