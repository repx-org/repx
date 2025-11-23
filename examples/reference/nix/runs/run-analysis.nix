{ pkgs, repx-lib }:
{
  name = "analysis-run";

  pipelines = [
    ./pipelines/pipe-analysis.nix
  ];

  params = { };
}
