import argparse
import json
import sys
from pathlib import Path

from repx_py.models import Experiment


def main():
    """Main function to parse arguments and run the parameter tracing."""
    parser = argparse.ArgumentParser(
        description="Trace effective parameters for all jobs in a lab directory.",
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        "lab_directory",
        type=Path,
        help="Path to the lab directory containing metadata.json or a 'revision' folder.",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        help="Path to the output JSON file. If not provided, prints to standard output.",
    )
    args = parser.parse_args()

    try:
        print(f"Loading experiment from: {args.lab_directory}", file=sys.stderr)
        exp = Experiment(args.lab_directory)

        print("Calculating effective parameters for all jobs...", file=sys.stderr)
        all_params = exp.effective_params
        print("Calculation complete.", file=sys.stderr)

    except (FileNotFoundError, KeyError, RecursionError) as e:
        print(f"\nError: {e}", file=sys.stderr)
        sys.exit(1)

    if args.output:
        with open(args.output, "w") as f:
            json.dump(all_params, f, indent=2)
        print(
            f"\nSuccessfully wrote effective parameters to '{args.output}'",
            file=sys.stderr,
        )
    else:
        # Print to stdout by default
        print(json.dumps(all_params, indent=2))


if __name__ == "__main__":
    main()
