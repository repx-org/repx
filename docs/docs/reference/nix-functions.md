# Nix Functions Reference

This reference documents the public API exposed by `repx.lib` (typically accessed as `repx-lib` in your flake).

## Lab Definition

### `repx-lib.mkLab`

Creates the top-level Lab derivation. This is the entry point for your experiment definition.

```nix
repx-lib.mkLab {
  inherit pkgs repx-lib;
  gitHash = self.rev or self.dirtyRev or "unknown";
  lab_version = "1.0.0";
  runs = { ... };
  groups = { ... };  # optional
}
```

**Arguments:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pkgs` | Attribute Set | Yes | The Nixpkgs package set. |
| `repx-lib` | Attribute Set | Yes | The RepX library instance. |
| `gitHash` | String | Yes | Git commit hash for provenance tracking. Baked into all metadata. Typically `self.rev or self.dirtyRev or "unknown"`. |
| `lab_version` | String | Yes | User-defined version string for this lab. Written into the lab manifest. |
| `runs` | Attribute Set | Yes | Dictionary where keys are run attribute names and values are run placeholders (created by `callRun`). |
| `groups` | Attribute Set | No (default: `{}`) | Named groupings of runs. See [Run Groups](#run-groups). |

**Returns:** A Lab derivation containing the complete experiment graph.

**Validation rules:**
- All run names must be unique after evaluation.
- Group names must not collide with any run name.
- Circular dependencies between runs cause a build error.

### `repx-lib.callRun`

Creates a run placeholder for use in `mkLab`'s `runs` attribute set. Run placeholders are resolved lazily during lab evaluation.

```nix
repx-lib.callRun <runPath> <dependencies>
```

**Arguments:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `runPath` | Path or Function | Path to a run definition file (`.nix`), or a function that returns a run definition. |
| `dependencies` | List | A list of dependency specifications. Can be empty (`[]`) for runs with no dependencies. |

Each element in the `dependencies` list can be:

- **A bare run placeholder** -- implies a `"hard"` dependency:
  ```nix
  repx-lib.callRun ./runs/analysis.nix [ simulationRun ]
  ```
- **A list `[ runPlaceholder "type" ]`** -- explicit dependency type (`"hard"` or `"soft"`):
  ```nix
  repx-lib.callRun ./runs/analysis.nix [ [ simulationRun "hard" ] [ validationRun "soft" ] ]
  ```

**Hard vs Soft dependencies:**
- **`"hard"`**: The dependent run receives all jobs from the dependency as inputs. The dependency must complete before the dependent run starts.
- **`"soft"`**: The dependent run is aware of the dependency's jobs but does not receive them as direct inputs.

**Returns:** An attribute set with `_repx_type = "run_placeholder"`.

**Complete example:**

```nix
# nix/lab.nix
{ pkgs, repx-lib, ... }:
let
  simulation = repx-lib.callRun ./runs/simulation.nix [];
  analysis   = repx-lib.callRun ./runs/analysis.nix [ simulation ];
  report     = repx-lib.callRun ./runs/report.nix [ [ analysis "hard" ] [ simulation "soft" ] ];
in
repx-lib.mkLab {
  inherit pkgs repx-lib;
  gitHash = "abc123";
  lab_version = "1.0.0";
  runs = {
    inherit simulation analysis report;
  };
}
```

### Run Groups

Groups allow you to tag collections of runs with a name for organizational purposes. Groups can be listed with `repx list groups`.

```nix
repx-lib.mkLab {
  # ...
  runs = {
    inherit training validation testing;
  };
  groups = {
    ml-pipeline = [ training validation ];
    evaluation = [ validation testing ];
  };
}
```

**Rules:**
- Each group value must be a list of run placeholders (created by `callRun`).
- Group names must not collide with any run name.

---

## Run Definition

### `repx-lib.mkRun` (internal)

Defines a parameterized run. You typically don't call `mkRun` directly -- instead, you write a run definition file that returns the arguments, and `callRun` + `mkLab` handle the rest.

A **run definition file** (e.g., `runs/simulation.nix`) returns an attribute set:

```nix
# nix/runs/simulation.nix
{ pkgs, repx-lib, ... }:
{
  name = "simulation";
  pipelines = [ ./pipelines/main.nix ];
  params = {
    seed = [ 1 2 3 ];
    model = [ "A" "B" ];
  };
}
```

**Attributes:**

| Attribute | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | String | Yes | | Unique name for this run. |
| `pipelines` | List of Paths | Yes | | Paths to pipeline definition files. |
| `params` | Attribute Set | Yes | | Parameter lists for sweeping. RepX generates the Cartesian product. |
| `containerized` | Boolean | No | `true` | When `false`, skips Docker/OCI image generation entirely. Use for native-only execution. |
| `paramsDependencies` | List | No | `[]` | Additional Nix derivations that parameter values depend on (beyond auto-detection). |

**Parameter format:**

Parameter values are lists. RepX computes the Cartesian product of all parameter lists:

```nix
params = {
  seed = [ 1 2 3 ];       # 3 values
  model = [ "A" "B" ];    # 2 values
};
# Produces 3 x 2 = 6 parameter combinations
```

The run definition file receives `{ pkgs, repx-lib, ... }` as arguments (via `callPackage`). You can access `repx-lib.utils` for parameter helpers -- see [mkUtils](#repx-libmkutils).

---

## Pipeline Construction

### `repx.mkPipe`

Constructs a pipeline from a set of stages. Used inside a pipeline definition file. `mkPipe` is essentially an identity function on the stages attribute set -- its purpose is to mark the set as a pipeline and provide future extensibility.

```nix
# nix/pipelines/main.nix
{ repx, pkgs, ... }:
repx.mkPipe rec {
  generate = repx.callStage ./stages/generate.nix [];
  train    = repx.callStage ./stages/train.nix [ generate ];
  analyze  = repx.callStage ./stages/analyze.nix [ train ];
}
```

**Arguments:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `stages` | Attribute Set | A (typically `rec`) attribute set where each key is a stage name and value is a Stage derivation (returned by `callStage`). |

**Returns:** The stages attribute set (a Pipeline definition).

:::note
Pipeline files receive `{ repx, pkgs, ... }` as arguments. The `repx` object contains `mkPipe` and `callStage`. This is distinct from `repx-lib` which is available in run definition files and lab files.
:::

### `repx.callStage`

Instantiates a stage from a file, resolving dependencies and parameters.

```nix
repx.callStage <path> <dependencies>
```

**Arguments:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `path` | Path | Path to the stage definition file (`.nix`). |
| `dependencies` | List | A list of stage dependencies. |

Each element in the `dependencies` list can be:

- **A Stage derivation** -- implicit mapping. Output names of the upstream stage are matched to input names of the current stage:
  ```nix
  train = repx.callStage ./stages/train.nix [ generate ];
  ```
- **A list `[ stage "source" "target" ]`** -- explicit mapping. Maps the `source` output of `stage` to the `target` input of the current stage:
  ```nix
  analyze = repx.callStage ./stages/analyze.nix [
    [ train "model_weights" "weights_file" ]
    [ generate "data_csv" "input_data" ]
  ];
  ```

**Returns:** A Stage derivation with `passthru` metadata.

---

## Stage Schema

Stages are defined as Nix functions that accept `{ pkgs }` and return an attribute set. There are two stage types: **simple** and **scatter-gather**.

### Common Attributes

These attributes are valid for both stage types:

| Attribute | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `pname` | String or Function | Yes | | Stage name. Can be a function `{ params }: ...` for dynamic names. |
| `version` | String | No | `"1.1"` | Stage version string. |
| `params` | Attribute Set | No | `{}` | Default parameter values. Overridden by run-level parameters of the same name. |
| `runDependencies` | List | No | `[]` | Nix packages to include in `$PATH` at runtime. |
| `resources` | Attribute Set or Function | No | `null` | Resource hints for SLURM scheduling. See [Resource Hints](#resource-hints). |
| `passthru` | Attribute Set | No | `{}` | Arbitrary attributes passed through to the derivation's `passthru`. |

### Simple Stage Attributes

In addition to common attributes:

| Attribute | Type | Required | Description |
|-----------|------|----------|-------------|
| `inputs` | Attribute Set or Function | No | Map of input identifiers to default values. Available as `$inputs` associative array in the script. Can be a function `{ params }: ...`. |
| `outputs` | Attribute Set or Function | No | Map of output identifiers to file path templates using `$out`. Can be a function `{ params }: ...`. |
| `run` | Function | Yes | The execution script. Receives `{ inputs, outputs, params, pkgs, ... }` and returns a Bash string. |

### Scatter-Gather Stage Attributes

In addition to common attributes:

| Attribute | Type | Required | Description |
|-----------|------|----------|-------------|
| `scatter` | Attribute Set | Yes | The scatter phase definition (has `inputs`, `outputs`, `run`, and optionally `resources`). |
| `steps` | Attribute Set | Yes | A set of step definitions forming a mini-DAG per branch. Each step has `pname`, `inputs`, `outputs`, `run`, `deps`, and optionally `resources` and `runDependencies`. See [Step Dependencies](#step-dependencies). |
| `gather` | Attribute Set | Yes | The gather phase definition (has `inputs`, `outputs`, `run`, and optionally `resources`). |
| `inputs` | Attribute Set | No | Shared inputs for the scatter phase. |

#### Step Dependencies

Each step in the `steps` attrset is an attribute set with these fields:

| Attribute | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `pname` | String | Yes | | Step name (must match the attrset key). |
| `inputs` | Attribute Set | Yes | | Map of input identifiers to default values. Root steps should declare `worker__item` to receive the scatter work item. |
| `outputs` | Attribute Set | Yes | | Map of output identifiers to `$out/...` path templates. |
| `deps` | List | Yes | | List of step references this step depends on. Empty list (`[]`) = root step. |
| `run` | Function | Yes | | Execution script, same contract as simple stages. |
| `resources` | Attribute Set | No | `null` | Per-step resource hints for SLURM scheduling. |
| `runDependencies` | List | No | `[]` | Additional Nix packages for this step's `$PATH`. |

**Dependency wiring** uses the same syntax as `repx.callStage` dependencies:

- **Bare reference** (`[ other_step ]`): Implicit name mapping — output names of the dependency are matched to input names of this step.
- **Explicit mapping** (`[ other_step "source_output" "target_input" ]`): Maps a specific output of the dependency to a specific input of this step.

**Constraints:**

- There must be exactly **one sink step** (a step no other step depends on). The sink step's outputs become the gather phase's inputs.
- At least one root step (`deps = []`) must declare a `worker__item` input.
- The step DAG must be acyclic.

### Dynamic Attribute Resolution

The `pname`, `inputs`, `outputs`, and `resources` attributes can be **functions** that accept `{ params }` and return the resolved value. This allows stage definitions to adapt based on parameters:

```nix
{ pkgs }:
{
  pname = { params }: "train-${params.model}";

  inputs = { params }: {
    "data" = "input.csv";
  } // (if params.use_pretrained then {
    "pretrained_weights" = "weights.bin";
  } else {});

  outputs = { params }: {
    "model" = "$out/model-${params.model}.bin";
    "metrics" = "$out/metrics.csv";
  };

  resources = { params }: {
    mem = if params.dataset_size > 10000 then "64G" else "8G";
    cpus = if params.model == "large" then 8 else 4;
  };

  params = {
    model = "small";
    dataset_size = 1000;
    use_pretrained = false;
  };

  runDependencies = [ pkgs.python3 ];

  run = { inputs, outputs, params, ... }: ''
    python3 train.py \
      --model ${params.model} \
      --output "${outputs.model}"
  '';
}
```

### Resource Hints

Resource hints guide SLURM job submission. They can be specified at the stage level and are automatically merged from upstream dependencies.

```nix
resources = {
  mem = "16G";           # Memory (supports K, M, G, T suffixes)
  cpus = 4;              # CPU count
  time = "02:00:00";     # Wall time (HH:MM:SS, MM:SS, or raw seconds)
  partition = "gpu";     # SLURM partition
  sbatch_opts = [ "--gres=gpu:1" ];  # Extra sbatch options
};
```

| Field | Type | Description |
|-------|------|-------------|
| `mem` | String | Memory limit. Suffixes: `K` (KiB), `M` (MiB), `G` (GiB), `T` (TiB). |
| `cpus` | Integer | Number of CPUs. |
| `time` | String | Wall time limit. Formats: `HH:MM:SS`, `MM:SS`, or raw seconds. |
| `partition` | String | SLURM partition name. |
| `sbatch_opts` | List of Strings | Additional `sbatch` flags. |

**Merge semantics:** When a stage depends on upstream stages, resource hints are automatically merged:
- `mem`, `cpus`, `time`: The **maximum** across all inputs and the stage's own declaration is used.
- `partition`, `sbatch_opts`: The stage's own value takes precedence (**last-writer-wins**). If unset, the first dependency's value is used.

For scatter-gather stages, each sub-stage (`scatter`, `gather`) and each individual step can have its own `resources` attribute.

---

## Utility Functions

### `repx-lib.mkUtils`

A factory that creates a set of parameter utility functions. Available automatically inside run definition files as `repx-lib.utils` (injected by `mkLab`).

```nix
utils = repx-lib.mkUtils { inherit pkgs; };
```

### `utils.list`

Wraps a plain list into a RepX parameter object. Use this when you have a dynamically constructed list that should be treated as a parameter sweep dimension.

```nix
utils.list [ 1 2 3 ]
# Equivalent to setting params = { x = [ 1 2 3 ]; } directly
```

### `utils.range`

Generates a list of integers from `start` to `end` (inclusive). Wrapper around `pkgs.lib.range`.

```nix
utils.range 1 10
# [ 1 2 3 4 5 6 7 8 9 10 ]
```

### `utils.scan`

Scans a directory for entries matching criteria. Works at Nix evaluation time.

```nix
utils.scan {
  src = ./data;          # Path or derivation to scan
  type = "file";         # "any" (default), "file", or "directory"
  match = ".*\\.csv";    # Optional regex pattern to filter by name
}
```

**Arguments:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `src` | Path or Derivation | (required) | The directory to scan. |
| `type` | String | `"any"` | Entry type filter: `"any"`, `"file"`, or `"directory"`. |
| `match` | String or null | `null` | Regex pattern to filter entry names. |

**Returns:** A RepX parameter object (`{ _repx_param = true; values = [...]; context = [...]; }`). The `values` are absolute paths to matching entries. The `context` tracks derivation dependencies for Nix garbage collection safety.

**Behavior for store paths vs local paths:**
- **Local paths**: Uses `builtins.readDir` for fast evaluation.
- **Store paths / derivations**: Uses `find` in a build step (since `readDir` doesn't work on store paths).

### `utils.dirs`

Scans a source for subdirectories. Shorthand for `scan { type = "directory"; ... }` with an important optimization: for non-store local paths, each directory is wrapped in its own individual derivation for fine-grained Nix caching.

```nix
# Sweep over directories in ./submissions/
{ pkgs, repx-lib, ... }:
{
  name = "grading";
  pipelines = [ ./pipelines/grade.nix ];
  params = {
    submission = repx-lib.utils.dirs ./submissions;
  };
}
```

### `utils.files`

Scans a source for files only. Shorthand for `scan { type = "file"; ... }`.

```nix
params = {
  config = repx-lib.utils.files ./configs;
};
```

---

## Script Execution Contract

When RepX executes a stage script, the following contract applies:

1. **Shell settings**: `set -euxo pipefail` -- scripts fail on any error, undefined variable, or pipe failure.
2. **Arguments**: The script receives `$1` as the output directory (`$out`) and `$2` as the inputs JSON manifest.
3. **Input readiness**: RepX polls for all input files to become readable with a **30-second timeout** (2-second intervals). This handles async filesystem syncs on networked storage.
4. **Output cleanup**: The output directory (`$out`) is cleared before each run (preserving `slurm-*.out` files).
5. **Working directory**: The script's working directory is set to `$out`.
6. **`$PATH`**: Contains only packages from `runDependencies` plus core utilities (bash, coreutils, findutils, sed, grep, jq).

---

## Build-Time Script Validation

Every stage script undergoes automatic validation at build time:

1. **ShellCheck**: The script is linted with `shellcheck` to catch common Bash issues.
2. **OSH parsing**: The script is parsed into an AST using [Oils for Unix (OSH)](https://www.oilshell.org/).
3. **Dependency analysis**: A Python analyzer walks the AST to extract all external command invocations and verifies each command exists in `$PATH` (populated by `runDependencies`).

If any command referenced in your script is not provided by `runDependencies`, **the Nix build fails** with an error listing the missing commands. This catches dependency issues at build time rather than at runtime.
