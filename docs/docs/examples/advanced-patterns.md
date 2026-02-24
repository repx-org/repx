# Advanced Patterns

This page covers advanced RepX patterns for complex experiment workflows.

## Dynamic Stages

Stage attributes can be functions of `{ params }`, enabling stages that adapt their behavior based on parameter values. This is useful when the number or names of inputs/outputs depend on parameters.

```nix
# stages/train.nix
{ pkgs }:
{
  pname = { params }: "train-${params.model}";

  params = {
    model = "resnet";
    epochs = 100;
  };

  inputs = { params }: {
    "data" = "input.csv";
  } // (if params.model == "pretrained" then {
    "pretrained_weights" = "weights.bin";
  } else {});

  outputs = { params }: {
    "model" = "$out/model-${params.model}.pt";
    "metrics" = "$out/metrics.csv";
  };

  resources = { params }: {
    mem = if params.model == "large" then "64G" else "8G";
    cpus = if params.model == "large" then 8 else 4;
    time = if params.epochs > 500 then "24:00:00" else "04:00:00";
  };

  runDependencies = [ pkgs.python3 ];

  run = { inputs, outputs, params, ... }: ''
    python3 train.py \
      --model ${params.model} \
      --epochs ${toString params.epochs} \
      --output "${outputs.model}"
  '';
}
```

The `pname`, `inputs`, `outputs`, and `resources` attributes are resolved at evaluation time with the concrete parameter values for each job instance.

## Directory Scanning with `utils`

The `repx-lib.utils` functions enable parameter sweeps over filesystem contents. This is useful for grading submissions, processing datasets, or running experiments across configuration files.

### Sweeping Over Directories

```nix
# nix/runs/grading.nix
{ pkgs, repx-lib, ... }:
{
  name = "grading";
  pipelines = [ ./pipelines/grade.nix ];
  params = {
    # Each subdirectory in ./submissions/ becomes a parameter value
    submission = repx-lib.utils.dirs ./submissions;
  };
}
```

For local paths, `utils.dirs` wraps each directory in its own Nix derivation. This means if you add a new submission, only the new directory triggers a rebuild -- existing submissions are cached.

### Sweeping Over Files

```nix
params = {
  config = repx-lib.utils.files ./configs;
};
```

### Advanced Scanning

Use `utils.scan` for more control:

```nix
params = {
  # Only CSV files
  dataset = repx-lib.utils.scan {
    src = ./data;
    type = "file";
    match = ".*\\.csv";
  };

  # Only directories matching a pattern
  experiment = repx-lib.utils.scan {
    src = ./experiments;
    type = "directory";
    match = "exp-[0-9]+";
  };
};
```

### Dynamic Lists

Use `utils.list` to wrap dynamically constructed lists:

```nix
{ pkgs, repx-lib, ... }:
let
  utils = repx-lib.utils;
  seeds = utils.list (map (x: x * 10) (utils.range 1 5));
  # seeds.values = [ 10 20 30 40 50 ]
in
{
  name = "sweep";
  pipelines = [ ./pipelines/main.nix ];
  params = {
    seed = seeds;
    model = [ "A" "B" ];
  };
}
```

## Run Groups

Groups let you organize runs into named collections without affecting execution. Useful for large experiments with many runs.

```nix
# nix/lab.nix
{ pkgs, repx-lib, ... }:
let
  preprocess = repx-lib.callRun ./runs/preprocess.nix [];
  train      = repx-lib.callRun ./runs/train.nix [ preprocess ];
  evaluate   = repx-lib.callRun ./runs/evaluate.nix [ train ];
  visualize  = repx-lib.callRun ./runs/visualize.nix [ evaluate ];
  baseline   = repx-lib.callRun ./runs/baseline.nix [ preprocess ];
in
repx-lib.mkLab {
  inherit pkgs repx-lib;
  gitHash = self.rev or self.dirtyRev or "unknown";
  lab_version = "1.0.0";
  runs = { inherit preprocess train evaluate visualize baseline; };
  groups = {
    ml-pipeline = [ preprocess train evaluate ];
    reporting = [ visualize ];
    baselines = [ baseline ];
  };
}
```

```bash
# List all groups
repx list groups

# List runs in a specific group
repx list groups ml-pipeline
```

## Resource Hints

Define resource requirements at the stage level for SLURM scheduling:

```nix
{ pkgs }:
{
  pname = "gpu-training";

  resources = {
    mem = "32G";
    cpus = 8;
    time = "12:00:00";
    partition = "gpu";
    sbatch_opts = [ "--gres=gpu:2" "--constraint=a100" ];
  };

  runDependencies = [ pkgs.python3 pkgs.cudatoolkit ];

  run = { outputs, params, ... }: ''
    python3 train_gpu.py --output "${outputs.model}"
  '';

  outputs = { "model" = "$out/model.pt"; };
}
```

Resources are automatically **merged from upstream dependencies**: if your stage depends on a stage that requires 16G memory and your stage requires 32G, the final resource hint is 32G (the maximum). For `partition` and `sbatch_opts`, the stage's own value takes precedence.

For scatter-gather stages, each sub-stage can define its own resource hints:

```nix
{ pkgs }:
{
  pname = "parallel-processing";

  scatter = {
    resources = { mem = "4G"; cpus = 1; time = "00:05:00"; };
    # ...
  };

  worker = {
    resources = { mem = "16G"; cpus = 4; time = "02:00:00"; partition = "compute"; };
    # ...
  };

  gather = {
    resources = { mem = "8G"; cpus = 2; time = "00:30:00"; };
    # ...
  };
}
```

## Impure Builds

While RepX encourages purity, practical research often requires "impure" access to the host system, such as:
*   Large datasets stored on a shared cluster filesystem (Lustre/GPFS).
*   Proprietary license servers.
*   Hardware-specific drivers.

You can configure impure mounts in `config.toml`:

```toml
[targets.cluster]
mount_host_paths = true
```

Or mount specific paths only:

```toml
[targets.cluster]
mount_paths = ["/data/shared/imagenet", "/opt/licenses"]
```

At the stage level, you can mark a stage as impure with `__noChroot = true` in the Nix derivation. See the [impure-incremental example](./impure-incremental.md) for a complete walkthrough.

:::warning
Impure builds sacrifice strict reproducibility for convenience. Always document external dependencies and consider whether the impure access is truly necessary.
:::

## Incremental Builds

RepX leverages Nix's content-addressed caching. If you change a parameter in a downstream stage, only that stage and its dependents will be rebuilt. Upstream stages that haven't changed are reused from the cache.

At runtime, `repx run` tracks job completion state. Subsequent invocations skip completed jobs unless `--force` is specified. Combined with `--continue-on-failure`, this enables efficient iterative development:

```bash
# First run -- some jobs may fail
repx run simulation --continue-on-failure

# Fix the issue, then re-run -- only failed/pending jobs execute
repx run simulation
```

## Inter-Run Dependencies

Runs can depend on other runs, enabling multi-stage experiment pipelines:

```nix
let
  data = repx-lib.callRun ./runs/data-generation.nix [];
  train = repx-lib.callRun ./runs/training.nix [ data ];
  eval = repx-lib.callRun ./runs/evaluation.nix [
    [ train "hard" ]  # must complete before eval starts
    [ data "soft" ]   # eval is aware of data but doesn't receive it as input
  ];
in
# ...
```

**Hard dependencies** pass all jobs from the upstream run as inputs to the downstream run's pipelines. **Soft dependencies** make the downstream run aware of the upstream run's jobs (for metadata/provenance) without creating data flow edges.

## Native-Only Runs

If your experiment doesn't need container isolation (e.g., it only uses host tools or runs on a trusted cluster), disable container image generation:

```nix
# nix/runs/lightweight.nix
{ pkgs, ... }:
{
  name = "lightweight";
  containerized = false;  # Skip Docker image generation
  pipelines = [ ./pipelines/quick.nix ];
  params = { seed = [ 1 2 3 ]; };
}
```

This reduces build time and Lab size significantly.
