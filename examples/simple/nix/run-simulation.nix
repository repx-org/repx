{ pkgs, repx-lib }:
{
  name = "simulation-run";
  pipelines = [
    ./pipe-sim.nix
  ];
  params = { };
}
