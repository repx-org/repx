{ pkgs, repx-lib }:
{
  name = "analysis-run";

  pipelines = [
    ./pipe-analysis.nix
  ];

  params = { };
}
