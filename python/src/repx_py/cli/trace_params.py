import argparse
import json
import logging
import sys
from pathlib import Path

from repx_py.models import Experiment

logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stderr)
logger = logging.getLogger("trace-params")


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
        logger.info(f"Loading experiment from: {args.lab_directory}")
        exp = Experiment(args.lab_directory)

        logger.info("Calculating effective parameters for all jobs...")
        all_params = exp.effective_params
        logger.info("Calculation complete.")

    except (FileNotFoundError, KeyError, RecursionError) as e:
        logger.error(f"\nError: {e}")
        sys.exit(1)

    if args.output:
        with open(args.output, "w") as f:
            json.dump(all_params, f, indent=2)
        logger.info(f"\nSuccessfully wrote effective parameters to '{args.output}'")
    else:
        print(json.dumps(all_params, indent=2))


if __name__ == "__main__":
    main()
