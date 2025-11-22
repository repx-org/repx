{ pkgs }:
{
  pname = "stage-analysis";

  inputs = {
    "store__base" = "";
    "metadata__simulation-run" = "";
  };

  outputs = {
    "analysis.plot" = "$out/plot.png";
  };

  runDependencies = [
    (pkgs.python3.withPackages (ps: [
      ps.pandas
      ps.matplotlib
    ]))
  ];

  run =
    { inputs, outputs, ... }:
    let
      analysisScript = pkgs.writeText "analysis_script.py" ''
        import argparse
        import json
        import sys
        import os
        import matplotlib.pyplot as plt
        from pathlib import Path

        def main():
            parser = argparse.ArgumentParser()
            parser.add_argument("--meta", required=True)
            parser.add_argument("--store", required=True)
            parser.add_argument("--output", required=True)
            args = parser.parse_args()

            print(f"Loading metadata from: {args.meta}")

            try:
                with open(args.meta, 'r') as f:
                    data = json.load(f)
            except Exception as e:
                print(f"Failed to load metadata: {e}")
                sys.exit(1)

            jobs = data.get('jobs', {})
            target_job_id = None
            target_job_data = None

            print("Searching for 'stage-C-consumer'...")
            for jid, jdata in jobs.items():
                pname = jdata.get('name', "") or ""
                if "stage-C-consumer" in pname:
                    target_job_id = jid
                    target_job_data = jdata
                    break

            if not target_job_id:
                print("Error: No job matching 'stage-C-consumer' found.")
                sys.exit(1)

            print(f"Found job: {target_job_id}")

            try:
                outputs_def = target_job_data.get('executables', {}).get('main', {}).get('outputs', {})
                if not outputs_def:
                     outputs_def = target_job_data.get('outputs', {})

                template = outputs_def.get("data.combined_list")
                if not template:
                    raise KeyError("data.combined_list")
            except KeyError:
                print("Error: Job does not have output 'data.combined_list'")
                sys.exit(1)

            filename = template.replace("$out/", "")
            data_path = Path(args.store) / "outputs" / target_job_id / "out" / filename

            print(f"Reading data from: {data_path}")

            try:
                with open(data_path, 'r') as f:
                    lines = f.readlines()
                    numbers = [int(x.strip()) for x in lines if x.strip()]
            except FileNotFoundError:
                print(f"Error: Data file not found at {data_path}")
                sys.exit(1)

            print(f"Plotting {len(numbers)} numbers...")
            plt.figure(figsize=(10, 6))
            plt.plot(numbers, marker='o', linestyle='-')
            plt.title(f"Reference Analysis (Raw Python) of {target_job_id}")
            plt.xlabel("Index")
            plt.ylabel("Value (Sum)")
            plt.grid(True)

            plt.savefig(args.output)
            print(f"Plot saved to {args.output}")

        if __name__ == "__main__":
            main()
      '';
    in
    ''
      python3 ${analysisScript} \
        --meta "${inputs."metadata__simulation-run"}" \
        --store "${inputs."store__base"}" \
        --output "${outputs."analysis.plot"}"
    '';
}
