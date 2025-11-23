{ pkgs, repx-lib }:
{
  name = "plot-run";
  pipelines = [ ./pipe-plot.nix ];
  params = { };
}
