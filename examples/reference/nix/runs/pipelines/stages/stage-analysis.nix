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
        import matplotlib.pyplot as plt
        from pathlib import Path

        def main():
            parser = argparse.ArgumentParser()
            parser.add_argument("--meta", required=True)
            parser.add_argument("--store", required=True)
            parser.add_argument("--output", required=True)
            args = parser.parse_args()

            with open(args.meta, 'r') as f:
                data = json.load(f)

            jobs = data.get('jobs', {})
            results = []

            store_path = Path(args.store)
            outputs_dir = store_path / "outputs"

            print(f"Scanning for results in: {outputs_dir}")

            for jid, jdata in jobs.items():
                if "stage-E-total-sum" in jdata.get('name', ""):

                    out_defs = jdata.get('executables', {}).get('main', {}).get('outputs', {})

                    if 'data.total_sum' in out_defs:
                        raw_out_path = out_defs['data.total_sum']
                        if raw_out_path.startswith("$out/"):
                            rel_path = raw_out_path[5:]
                        else:
                            rel_path = raw_out_path.replace("$out/", "")

                        job_output_dir = outputs_dir / jid

                        if not job_output_dir.exists():
                             candidates = list(outputs_dir.glob(f"{jid}-*"))
                             if candidates:
                                 job_output_dir = candidates[0]

                        full_path = job_output_dir / "out" / rel_path

                        if full_path.exists():
                             try:
                                 with open(full_path) as f:
                                     val = f.read().strip()
                                     if val:
                                         results.append(float(val))
                             except ValueError:
                                 print(f"Skipping invalid number in {full_path}")
                        else:
                            pass

            print(f"Total results found: {len(results)}")

            plt.figure()
            if results:
                plt.hist(results, bins=10)
                plt.title(f"Histogram of Total Sums (N={len(results)})")
                plt.xlabel("Total Sum")
                plt.ylabel("Frequency")
            else:
                plt.text(0.5, 0.5, 'No Data Found', ha='center', va='center')
                plt.title("Histogram (Empty)")

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
