{ pkgs, repx-lib }:
{
  name = "sweep-run";

  pipelines = [ ./pipe-sweep.nix ];

  params = {
    slope = [
      1
      2
      5
    ];
  };
}
