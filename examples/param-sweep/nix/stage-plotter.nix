{ pkgs }:
{
  pname = "plotter";

  inputs = {
    "store__base" = "";
    "metadata__sweep-run" = "";
  };

  outputs = {
    "plot.png" = "$out/combined_plot.png";
  };

  runDependencies = [
    (pkgs.python3.withPackages (ps: [
      ps.pandas
      ps.matplotlib
      pkgs.repx-py
    ]))
  ];

  run =
    { inputs, outputs, ... }:
    let
      script = pkgs.writeText "plot.py" ''
        import argparse
        import matplotlib.pyplot as plt
        from repx_py import Experiment

        parser = argparse.ArgumentParser()
        parser.add_argument("--meta", required=True)
        parser.add_argument("--store", required=True)
        parser.add_argument("--out", required=True)
        args = parser.parse_args()

        exp = Experiment.from_run_metadata(args.meta, args.store)

        jobs = exp.jobs().filter(name__contains="generator")

        plt.figure(figsize=(10, 6))

        for job in jobs:
            slope = job.effective_params.get("slope")

            df = job.load_csv("data.csv")

            plt.plot(df['x'], df['y'], label=f"Slope = {slope}")

        plt.title("Parameter Sweep: y = slope * x^2")
        plt.xlabel("x")
        plt.ylabel("y")
        plt.legend()
        plt.grid(True)

        plt.savefig(args.out)
        print(f"Plot saved to {args.out}")
      '';
    in
    ''
      python3 ${script} \
        --meta "${inputs."metadata__sweep-run"}" \
        --store "${inputs."store__base"}" \
        --out "${outputs."plot.png"}"
    '';
}
