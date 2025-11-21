{ pkgs, repx-lib }:
{
  name = "simulation-run";
  containerized = false; # Keeping it simple/fast

  pipelines = [
    ./pipe-sim.nix
  ];

  params = { };
}
