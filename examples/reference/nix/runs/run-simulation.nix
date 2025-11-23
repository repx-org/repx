{ pkgs, repx-lib }:
{
  name = "simulation-run";

  pipelines = [
    ./pipelines/pipe-simulation.nix
  ];

  params = { };
}
