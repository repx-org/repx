{
  repx-lib,
  ...
}:
let
  inherit (repx-lib) utils;
in
{
  name = "simulation-run";
  containerized = false;
  pipelines = [ ./pipelines/pipe-simulation.nix ];

  parameters = {
    offset = utils.range 1 2;
    mode = utils.list [
      "fast"
      "slow"
    ];
    template_dir = utils.dirs ../pkgs/headers;
    config_file = utils.scan {
      src = ../pkgs/configs;
      match = ".*\\.json";
      type = "file";
    };
    config = utils.zip {
      multiplier = utils.range 2 3;
      scale = [
        10
        100
      ];
    };
  };
}
