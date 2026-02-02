# Containerization

RepX ensures reproducibility by running every job in an isolated container environment. This prevents host system libraries from leaking into your experiment and ensures that dependencies defined in Nix are the only ones available.

## Isolation Mechanism

By default, RepX uses **Bubblewrap (`bwrap`)** for local execution and on clusters where it is available. If configured, it can also use **Docker**, **Podman**, or **Apptainer/Singularity**.

The container environment:
*   **Read-only Root:** The root filesystem is read-only.
*   **Mounts:**
    *   `/nix/store`: Mounted read-only (so Nix packages work).
    *   `$out`: Mounted read-write (the job's output directory).
    *   Inputs: Specific input files are mounted into the container.
*   **Networking:** Networking is disabled by default to ensure purity (can be enabled via configuration).

## Configuring Runtimes

You can specify the container runtime in `config.toml` or per-target.

```toml
[execution]
isolation = "bwrap" # options: "bwrap", "docker", "podman", "process" (no isolation)
```

## HPC Considerations

On HPC systems, `bwrap` is often preferred because it doesn't require root privileges (unlike Docker). Apptainer (Singularity) is also a common target for RepX which supports converting the Nix closure into a SIF image on the fly (feature in progress).

## Debugging

If you need to inspect the environment of a job, you can use `repx debug-run`:

```bash
repx debug-run <job_id> --lab ./result
```

This drops you into a shell *inside* the container environment, exactly as the job would see it.
