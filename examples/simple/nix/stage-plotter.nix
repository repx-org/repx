{ pkgs }:
{
  pname = "simple-plotter";

  inputs = {
    "store__base" = "";
    "metadata__simulation-run" = "";
  };

  outputs = {
    "analysis.png" = "$out/plot.png";
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
        import pandas as pd
        from repx_py import Experiment

        parser = argparse.ArgumentParser()
        parser.add_argument("--meta", required=True)
        parser.add_argument("--store", required=True)
        parser.add_argument("--out", required=True)
        args = parser.parse_args()

        # 1. Initialize Experiment from the upstream run's metadata
        print(f"Loading experiment from {args.meta}")
        exp = Experiment.from_run_metadata(args.meta, args.store)

        # 2. Retrieve data from the 'producer' job
        print("Fetching producer data...")
        producer_job = exp.jobs().filter(name__contains="simple-producer")[0]

        # We can load the CSV directly using the helper
        df = producer_job.load_csv("data.csv")

        # 3. Retrieve data from the 'summer' job
        print("Fetching summer data...")
        summer_job = exp.jobs().filter(name__contains="simple-summer")[0]

        # Get the path to the text file
        sum_path = summer_job.get_output_path("result.sum")
        with open(sum_path, 'r') as f:
            total_sum = float(f.read().strip())

        # 4. Plot
        print(f"Plotting... (Total Sum: {total_sum})")
        plt.figure(figsize=(10, 6))
        plt.plot(df['x'], df['y'], label='Generated Data')
        plt.fill_between(df['x'], df['y'], alpha=0.3)
        plt.title(f"Simulation Analysis\nCalculated Area (Sum): {total_sum:.4f}")
        plt.xlabel("X")
        plt.ylabel("Y")
        plt.legend()
        plt.grid(True)

        plt.savefig(args.out)
        print(f"Saved to {args.out}")
      '';
    in
    ''
      python3 ${script} \
        --meta "${inputs."metadata__simulation-run"}" \
        --store "${inputs."store__base"}" \
        --out "${outputs."analysis.png"}"
    '';
}
