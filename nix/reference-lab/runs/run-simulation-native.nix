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

  params = {
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
    multiplier = utils.range 2 3;
  };
}
