# Garbage Collection

RepX caches lab artifacts, container images, and job outputs on each target. Over time, this accumulates disk usage from old experiments. The garbage collection (GC) system reclaims space by deleting artifacts and outputs that are no longer referenced by any GC root.

## How GC Roots Work

A **GC root** is a symlink that points to a lab's metadata file in the artifact store. Any artifact or job output reachable from a GC root is considered "live" and will not be deleted.

There are two kinds of GC roots:

### Auto Roots

Created automatically on every `repx run` submission. Auto roots are organized by project and subject to a rotation policy that keeps the **last 5** per project.

A "project" is identified by a hash of the git remote URL and the absolute lab path, so different checkouts of the same repo share the same project ID.

### Pinned Roots

Created explicitly by the user via `repx gc pin` or the TUI. Pinned roots are **never automatically removed** -- they survive indefinitely until manually unpinned.

## Directory Structure

```
<base_path>/
  gcroots/
    auto/
      <project_id>/
        <timestamp>_<lab_hash>  ->  ../../artifacts/lab/<hash>-lab-metadata.json
        ...                         (max 5 per project, oldest rotated out)
    pinned/
      <name>                    ->  ../../artifacts/lab/<hash>-lab-metadata.json
  artifacts/
    bin/          (always preserved)
    lab/          (sub-entries GC'd individually)
    images/       (sub-entries GC'd individually)
    jobs/         (sub-entries GC'd individually)
    store/        (sub-entries GC'd individually)
    ...
  outputs/
    <job_id>/     (deleted if job not referenced by any live lab)
```

## What GC Deletes

When you run `repx gc`, the system:

1. Scans all GC roots (both auto and pinned) to determine which artifacts and job IDs are "live"
2. Deletes artifacts in collection directories (`lab/`, `images/`, `jobs/`, `store/`, `host-tools/`, etc.) that are not referenced by any live root
3. Deletes job output directories (`outputs/<job_id>/`) whose job ID is not present in any live lab
4. Never touches `bin/` (always preserved)

## CLI Usage

### Run Garbage Collection

```bash
repx gc [--target <name>]
```

Scans roots and deletes unreferenced artifacts and outputs on the specified target.

### List GC Roots

```bash
repx gc list [--target <name>]
```

Shows all auto and pinned roots with their names and symlink targets.

### Pin a Lab

```bash
# Pin the current lab (uses content hash from built lab metadata)
repx gc pin

# Pin a specific lab by hash
repx gc pin <lab_hash>

# Pin with a custom name
repx gc pin <lab_hash> --name my-experiment
```

If `--name` is not provided, the lab hash is used as the name. Pinning the same name again overwrites the previous pin.

### Unpin a Lab

```bash
repx gc unpin <name>
```

Removes the named pinned root. The lab's artifacts become eligible for GC on the next run.

## TUI Usage

In the TUI, press **Space** to open the Quick Actions menu, then **p** to toggle pin/unpin for the current lab on the active target.

When the lab is pinned, a green **[Pinned]** indicator appears in the overview panel title bar.

## Tips

- Pin important experiment results before running GC to ensure they're preserved
- Use `repx gc list` to audit what's protected before running `repx gc`
- Auto roots rotate per-project, so switching between experiments on the same project will eventually expire older roots
- GC is safe to run at any time -- it only deletes artifacts not reachable from any root
