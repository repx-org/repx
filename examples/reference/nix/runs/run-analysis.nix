{ pkgs, repx-lib }:
{
  name = "analysis-run";
  containerized = true;

  pipelines = [
    ./pipelines/pipe-analysis.nix
  ];

  params = { };
}
