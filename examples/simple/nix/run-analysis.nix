{ pkgs, repx-lib }:
{
  name = "analysis-run";
  containerized = false;

  pipelines = [
    ./pipe-analysis.nix
  ];

  params = { };
}
