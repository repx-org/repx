{
  pkgs,
  jobDerivations,
}:

let
  exampleJobDrv = if jobDerivations != [ ] then pkgs.lib.head jobDerivations else null;
  exampleJobId =
    if exampleJobDrv != null then builtins.baseNameOf (toString exampleJobDrv) else "your-job-id-here";

  ifJobs =
    content:
    if exampleJobDrv != null then
      content
    else
      ''
        *No jobs were defined in this build. The lab is empty.*
      '';

  nativeContent = ''
    # HPC Experiment Lab: How to Use

    This lab directory is a self-contained "seed" for your experiments. It contains all the job scripts, their dependencies, and metadata describing the experiment structure. It does **not** contain any orchestration tools. You are expected to use your own scripts or a workflow manager to execute the jobs in the correct order.

    The core components are:
    *   `jobs/`: A directory containing one subdirectory for each job package defined in Nix.
    *   `revision/*/metadata.json`: A file that describes all jobs, their dependencies (input mappings), and parameters. This is the "map" of your experiment.

    ---
    ## Manual Execution Workflow

    This is the fundamental way to run jobs from the lab.

    ${ifJobs ''
      ### 1. Understanding a Job

      First, inspect the `metadata.json` file to understand the dependency graph. You can use `jq` to explore it. Each job has an entry with its `input_mappings`.

      ### 2. Executing a Simple Job

      A simple job package (one that is not "scatter-gather") contains a single executable script in its `bin/` directory.

      1.  **Find the script:**
          ```bash
          # Example for one job:
          ls -l jobs/${exampleJobId}/bin/
          ```

      2.  **Execute the script:**
          The script expects two arguments: `(output_directory) (inputs.json_path)`.
          ```bash
          # Create a temporary output directory and an inputs file.
          # For a job with no dependencies, the inputs file can be an empty JSON object.
          OUT_DIR=$(mktemp -d)
          INPUTS_JSON=$(mktemp)
          echo "{}" > $INPUTS_JSON

          # Run the script
          jobs/${exampleJobId}/bin/* "$OUT_DIR" "$INPUTS_JSON"

          # View the results
          echo "Job finished. Results are in: $OUT_DIR"
          ls -l $OUT_DIR
          ```
      For a job that *does* have dependencies, you must first run its dependencies, find their output paths, and construct an `inputs.json` file that maps the `target_input` names to the corresponding output paths from the dependency jobs.

      ### 3. Executing a Scatter-Gather Job

      A scatter-gather stage is more complex and is split into three scripts. An external runner must orchestrate them.

      1.  **Find the scripts:** A job like `stage-D-partial-sums` will have three executables:
          ```bash
          $ ls jobs/stage-D-partial-sums-*/bin/
          stage-D-partial-sums-gather
          stage-D-partial-sums-scatter
          stage-D-partial-sums-worker
          ```

      2.  **Orchestration logic:** You must implement the following sequence:
          a.  **Run the `scatter` script.** It takes `(output_dir) (inputs.json) (scatter_results_file)`. It will write a JSON list of work items to `scatter_results_file`.
          b.  **Loop over the `scatter_results_file`**. For each JSON object (work item):
              i.  Create a unique temporary directory for the worker.
              ii. **Run the `worker` script.** It takes `(worker_out_dir) (stage_inputs.json) (worker_input.json) (worker_output_path_file)`. It reads the single work item from `worker_input.json` and writes the *path* to its own result file into `worker_output_path_file`.
          c.  **Collect all the worker output paths.** After all workers have finished successfully, create a new JSON file that is a list of all the paths written by the workers.
          d.  **Run the `gather` script.** It takes `(output_dir) (inputs.json) (worker_outputs_manifest.json)`. It reads the list of worker output paths and produces the final stage output.

    ''}
  '';

  containerContent = "";
  unifiedContainerContent = "";

in
{
  readmeNative = pkgs.writeTextFile {
    name = "readme-native";
    destination = "/README.md";
    text = nativeContent;
  };

  readmeContainer = pkgs.writeTextFile {
    name = "readme-container";
    destination = "/README_container.md";
    text = containerContent;
  };
  readmeContainerUnified = pkgs.writeTextFile {
    name = "readme-unified";
    destination = "/README_unified.md";
    text = unifiedContainerContent;
  };
}
