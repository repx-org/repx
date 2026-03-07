_: {
  name = "sweep-run";

  pipelines = [ ./pipe-sweep.nix ];

  parameters = {
    slope = [
      1
      2
      5
    ];
  };
}
