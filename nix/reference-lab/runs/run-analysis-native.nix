_: {
  name = "analysis-run";
  containerized = false;

  pipelines = [
    ./pipelines/pipe-analysis.nix
  ];

  params = { };
}
