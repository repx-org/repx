checkFilePath:
{
  repx-lib,
  ...
}:
let
  inherit (repx-lib) utils;
in
{
  name = "mount-paths-run";
  pipelines = [ ./pipelines/pipe-mount-paths.nix ];

  parameters = {
    check_path = utils.list [ checkFilePath ];
  };
}
