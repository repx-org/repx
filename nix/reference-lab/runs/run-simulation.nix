{
  repx-lib,
  pkgs,
  ...
}:
let
  inherit (repx-lib) utils;
in
{
  name = "simulation-run";
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
    workload_args = utils.list [
      "10 20 30"
      "5 15 25"
    ];
    env = [
      (utils.env { OMP_NUM_THREADS = "1"; })
    ];
    nix_tool_bin = [ "${pkgs.hello}/bin" ];
  };
}
