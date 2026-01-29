import argparse
import json
import logging
import os
import subprocess
import sys
from pathlib import Path

from repx_py.models import Experiment, JobView, LocalCacheResolver

logging.basicConfig(
    level=logging.INFO, format="[%(levelname)s] %(message)s", stream=sys.stderr
)
logger = logging.getLogger("debug-runner")


def find_writable_cache_dir() -> Path:
    """Traverses up from the CWD to find a writable directory for the job output cache."""
    try:
        current_dir = Path(os.environ.get("PWD", os.getcwd()))
    except KeyError:
        current_dir = Path(os.getcwd())

    for directory in [current_dir, *list(current_dir.parents)]:
        if os.access(directory, os.W_OK):
            cache_path = directory / ".repx-cache"
            logger.debug(f"Auto-detected writable job cache directory: {cache_path}")
            return cache_path

    raise OSError(
        "Could not find a writable directory for the job cache. Please specify --job-cache."
    )


def generate_logic_manifest(job: JobView, job_cache_dir: Path, output_json_path: Path):
    """Generates the logic manifest (inputs.json) for a given job."""
    final_json = {}
    for mapping in job.input_mappings:
        dep_id = mapping.get("job_id")
        source_output = mapping.get("source_output")
        target_input = mapping.get("target_input")

        if not all([dep_id, source_output, target_input]):
            continue

        if dep_id == "self":
            continue

        dep_job = job._exp.get_job(dep_id)
        value_template = dep_job.outputs.get(source_output)
        if not value_template:
            logger.error(
                f"Missing source output '{source_output}' for dependency '{dep_id}'."
            )
            sys.exit(1)

        final_path_str = value_template.replace(
            "$out", str(job_cache_dir / dep_id / "out")
        )
        final_json[target_input] = final_path_str

    with open(output_json_path, "w") as f:
        json.dump(final_json, f, indent=2)


def execute_job(
    job: JobView,
    lab_path: Path,
    job_cache_dir: Path,
):
    """Executes a single job using the simple (out_dir, inputs.json) contract."""
    if job.stage_type != "simple":
        logger.warning(f"SKIPPING Job: {job.id} (type: {job.stage_type})")
        (job_cache_dir / job.id / "repx" / "SUCCESS").mkdir(parents=True, exist_ok=True)
        (job_cache_dir / job.id / "repx" / "SUCCESS").touch()
        (job_cache_dir / job.id / "out").mkdir(parents=True, exist_ok=True)
        return

    logger.info(f"Executing Job: {job.id}")

    job_out_dir = job_cache_dir / job.id / "out"
    job_repx_dir = job_cache_dir / job.id / "repx"
    job_logic_manifest_path = job_repx_dir / "logic-manifest.json"
    job_out_dir.mkdir(parents=True, exist_ok=True)
    job_repx_dir.mkdir(exist_ok=True)

    generate_logic_manifest(job, job_cache_dir, job_logic_manifest_path)

    executable_rel_path = job.executable_path
    if not executable_rel_path:
        logger.error(
            f"Could not find main executable path for job '{job.id}' in metadata."
        )
        sys.exit(1)

    job_executable = lab_path / executable_rel_path
    if not job_executable.is_file():
        logger.error(f"Job script not found at expected path: '{job_executable}'")
        sys.exit(1)

    command = [
        str(job_executable),
        str(job_out_dir),
        str(job_logic_manifest_path),
    ]

    logger.debug(f"Command: {' '.join(command)}")
    result = subprocess.run(command, capture_output=True, text=True, check=False)

    if result.returncode != 0:
        logger.error(f"Job {job.id} failed with exit code {result.returncode}!")
        logger.error("--- STDOUT ---\n" + result.stdout)
        logger.error("--- STDERR ---\n" + result.stderr)
        sys.exit(1)

    (job_repx_dir / "SUCCESS").touch()

    logger.info(f"Finished Job: {job.id}")


def ensure_job_is_run(
    job: JobView,
    exp: Experiment,
    lab_path: Path,
    job_cache_dir: Path,
):
    """Recursively runs dependencies and then the job itself, with caching."""
    success_marker = job_cache_dir / job.id / "repx" / "SUCCESS"
    if success_marker.exists():
        return

    for dep_job in job.dependencies:
        ensure_job_is_run(
            dep_job,
            exp,
            lab_path,
            job_cache_dir,
        )

    execute_job(job, lab_path, job_cache_dir)


def main():
    try:
        current_dir = Path(os.environ.get("PWD", os.getcwd()))
    except KeyError:
        current_dir = Path(os.getcwd())

    parser = argparse.ArgumentParser(description="Debug runner for RepX experiments.")
    parser.add_argument("--job", required=True, help="The ID of the job to run.")
    parser.add_argument(
        "--lab", default=current_dir, type=Path, help="Path to the lab directory."
    )
    parser.add_argument(
        "--job-cache",
        type=Path,
        help="Directory for job outputs. Defaults to ./.repx-cache.",
    )
    args = parser.parse_args()

    job_id = Path(args.job).name
    lab_path = args.lab.resolve()
    job_cache_dir = (
        args.job_cache.resolve() if args.job_cache else find_writable_cache_dir()
    )

    try:
        exp = Experiment(lab_path, resolver=LocalCacheResolver(job_cache_dir))
    except FileNotFoundError as e:
        logger.error(f"Error: {e}")
        logger.error("Hint: Make sure the lab path is correct.")
        sys.exit(1)

    target_job = exp.get_job(job_id)

    logger.info(f"Starting debug run for job: {target_job.id}")
    job_cache_dir.mkdir(exist_ok=True)

    ensure_job_is_run(
        target_job,
        exp,
        lab_path,
        job_cache_dir,
    )
    logger.info("---")
    logger.info("Debug run finished successfully.")
    logger.info(
        f"Final output for job {target_job.id} is in: {job_cache_dir / target_job.id / 'out'}"
    )


if __name__ == "__main__":
    main()
