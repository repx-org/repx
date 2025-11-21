{ pkgs, repx-lib }:
{
  name = "simulation-run";
  containerized = true;

  pipelines = [
    ./pipelines/pipe-simulation.nix
  ];

  params = { };
}
