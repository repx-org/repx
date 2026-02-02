# Parameters

Parameters allow you to define variable inputs for your stages, enabling powerful parameter sweeps and reusable configurations.

## 1. Defining Parameters in Stages

Parameters are declared in the `params` attribute of a stage definition. These serve as **defaults**.

```nix
{ pkgs }:
{
  pname = "train-model";
  
  params = {
    learning_rate = 0.01;
    batch_size = 32;
    optimizer = "adam";
  };
  
  run = { params, ... }: ''
    # Parameters are available in the params object
    python3 train.py \
      --lr ${toString params.learning_rate} \
      --batch ${toString params.batch_size} \
      --opt "${params.optimizer}"
  '';
}
```

## 2. Sweeping Parameters in Runs

In your Run definition (`repx.mkRun`), you can override these defaults. By providing a list of values, you instruct RepX to perform a **Parameter Sweep**.

RepX uses **Dot Notation** (`StageName.ParamName`) to target specific parameters in the pipeline.

```nix
repx-lib.mkRun {
  name = "hyperparam-sweep";
  pipelines = [ ./pipe-train.nix ];
  
  params = {
    # Target the 'learning_rate' parameter of the 'train-model' stage
    "train-model.learning_rate" = [ 0.1 0.01 0.001 ];
    
    # Target the 'optimizer' parameter
    "train-model.optimizer" = [ "adam" "sgd" ];
  };
}
```

### Cartesian Product

RepX automatically generates the **Cartesian Product** of all parameter lists. In the example above, `3` learning rates * `2` optimizers = `6` distinct jobs will be created.

## 3. Dynamic Parameters (Advanced)

Sometimes you need to set parameters based on the output of a previous stage (e.g., hyperparameter optimization). This is handled via **Scatter-Gather** stages where the `scatter` step generates the dynamic configuration for the `worker` steps.

See the [Scatter-Gather documentation](./stages.md#scatter-gather-stage) for details.

## 4. Tracing Parameters

Since parameters can come from defaults, overrides, or upstream dependencies, it can be useful to see the *effective* configuration of a specific job.

Use the CLI to trace parameters:

```bash
repx trace-params <job_id> --lab ./result
```
