{ pkgs, repx-lib }:

repx-lib.mkRun {
  inherit pkgs repx-lib;

  name = "simulation-run";
  containerized = true;

  pipelines = [
    ./pipelines/pipe-simulation.nix
  ];

  # For this example, we focus on a single run of the pipeline
  # without a parameter sweep to make the result clear.
  params = { };
}
