import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict

from repx_py.models import Experiment, JobView

COMPLETED_JOBS = set()


def find_writable_cache_dir() -> Path:
    """Traverses up from the CWD to find a writable directory for the job output cache."""
    try:
        current_dir = Path(os.environ.get("PWD", os.getcwd()))
    except KeyError:
        current_dir = Path(os.getcwd())

    for directory in [current_dir] + list(current_dir.parents):
        if os.access(directory, os.W_OK):
            cache_path = directory / ".repx-cache"
            print(
                f"[DEBUG-RUNNER] Auto-detected writable job cache directory: {cache_path}",
                file=sys.stderr,
            )
            return cache_path

    raise OSError(
        "Could not find a writable directory for the job cache. Please specify --job-cache."
    )


def generate_logic_manifest(job: JobView, job_cache_dir: Path, output_json_path: Path):
    """Generates the logic manifest (formerly inputs.json) for a given job."""
    final_json = {}
    for mapping in job._data.get("input_mappings", []):
        dep_id = mapping.get("job_id")
        source_output = mapping.get("source_output")
        target_input = mapping.get("target_input")

        if not all([dep_id, source_output, target_input]):
            continue

        dep_job = job._exp.get_job(dep_id)
        value_template = dep_job._data.get("outputs", {}).get(source_output)
        if not value_template:
            print(
                f"ERROR: Missing source output '{source_output}' for dependency '{dep_id}'.",
                file=sys.stderr,
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
    runtime_manifest_data: Dict[str, Any],
):
    """Executes a single job using its dynamically-read entrypoint contract."""
    chosen_runtime = runtime_manifest_data.get("runtimeName", "native")
    print(f"--- [DEBUG-RUNNER] Executing Job: {job.id} (Runtime: {chosen_runtime}) ---")

    job_out_dir = job_cache_dir / job.id / "out"
    job_repx_dir = job_cache_dir / job.id / "repx"
    job_logic_manifest_path = job_repx_dir / "logic-manifest.json"
    job_out_dir.mkdir(parents=True, exist_ok=True)
    job_repx_dir.mkdir(exist_ok=True)

    generate_logic_manifest(job, job_cache_dir, job_logic_manifest_path)

    job_script_package_path = lab_path / job.path_in_lab
    job_executable = next(job_script_package_path.glob("bin/*"), None)
    if not job_executable:
        print(
            f"ERROR: Job script not found in '{job_script_package_path / 'bin'}'",
            file=sys.stderr,
        )
        sys.exit(1)

    # --- DYNAMIC MANIFEST GENERATION (DUAL MANIFEST) ---
    contract = job.entrypoint_contract
    if not contract:
        print(
            f"FATAL: Job '{job.id}' is missing 'entrypoint_contract' in its metadata.",
            file=sys.stderr,
        )
        sys.exit(1)

    # A "value store" of all possible variables a manifest might need.
    value_store = {
        "outDir": str(job_out_dir),
        "logicManifestPath": str(job_logic_manifest_path),
        "meatScriptName": f"../internal/{job.pname}-meat",
        "successMarkerDir": str(job_repx_dir),
        "isWorker": False,
        "stageOut": str(job_out_dir),
        # --- FIX: Provide default empty values for the new optional keys ---
        # This satisfies the dispatcher contract for all job types.
        "scatterOutputFile": "",
        "workerInputJsonFile": "",
        "workerOutputFile": "",
        "gatherFilesManifest": "",
    }

    # Build the logic manifest by picking only the keys required by this job's contract.
    entrypoint_logic_manifest_data = {}
    for key in contract.get("logic", []):
        if key not in value_store:
            print(
                f"FATAL: debug-runner does not know how to provide value for required logic contract key '{key}'",
                file=sys.stderr,
            )
            sys.exit(1)
        entrypoint_logic_manifest_data[key] = value_store[key]

    entrypoint_logic_manifest_path = job_repx_dir / "entrypoint-logic-manifest.json"
    with open(entrypoint_logic_manifest_path, "w") as f:
        json.dump(entrypoint_logic_manifest_data, f)

    # The runtime manifest is passed in directly.
    entrypoint_runtime_manifest_path = job_repx_dir / "entrypoint-runtime-manifest.json"
    with open(entrypoint_runtime_manifest_path, "w") as f:
        json.dump(runtime_manifest_data, f)
    # --- END DYNAMIC MANIFEST GENERATION ---

    command = [
        str(job_executable),
        str(entrypoint_logic_manifest_path),
        str(entrypoint_runtime_manifest_path),
    ]

    print(f"[DEBUG-RUNNER] Command: {' '.join(command)}", file=sys.stderr)
    result = subprocess.run(command, capture_output=True, text=True, check=False)

    if result.returncode != 0:
        print(
            f"ERROR: Job {job.id} failed with exit code {result.returncode}!",
            file=sys.stderr,
        )
        print("--- STDOUT ---", file=sys.stderr)
        print(result.stdout, file=sys.stderr)
        print("--- STDERR ---", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        sys.exit(1)

    print(f"--- [DEBUG-RUNNER] Finished Job: {job.id} ---")


def ensure_job_is_run(
    job: JobView,
    exp: Experiment,
    lab_path: Path,
    job_cache_dir: Path,
    user_runtime_choice: str | None,
    artifact_cache_dir: Path | None,
):
    """Recursively runs dependencies and then the job itself, handling runtime selection."""
    success_marker = job_cache_dir / job.id / "repx" / "SUCCESS"
    if success_marker.exists():
        return

    # 1. Handle dependencies first
    dep_ids = {
        m["job_id"] for m in job._data.get("input_mappings", []) if "job_id" in m
    }
    for dep_id in dep_ids:
        dep_job = exp.get_job(dep_id)
        ensure_job_is_run(
            dep_job,
            exp,
            lab_path,
            job_cache_dir,
            user_runtime_choice,
            artifact_cache_dir,
        )

    # 2. Determine which runtime and arguments to use for this job
    run_name, run_data = exp.get_run_for_job(job.id)
    available_runtimes: Dict[str, Any] = run_data.get("runtimes", {})

    runtime_manifest_data = {"runtimeName": "native"}  # Default

    if user_runtime_choice and user_runtime_choice != "native":
        # User specified a non-native runtime tool
        mapped_runtime = "bubblewrap" if user_runtime_choice == "bubblewrap" else "oci"
        if mapped_runtime in available_runtimes:
            runtime_manifest_data["runtimeName"] = mapped_runtime
            artifact_path = (
                lab_path / available_runtimes[mapped_runtime]["artifact_path"]
            )
            if mapped_runtime == "oci":
                runtime_manifest_data["ociTool"] = user_runtime_choice
                runtime_manifest_data["ociImageArtifact"] = artifact_path.name
            elif mapped_runtime == "bubblewrap":
                runtime_manifest_data["bwrapSandboxArtifact"] = artifact_path.name
                runtime_manifest_data["bwrapCacheRoot"] = str(artifact_cache_dir)
        else:
            print(
                f"ERROR: Runtime tool '{user_runtime_choice}' is not supported for job '{job.id}'.",
                file=sys.stderr,
            )
            print(
                f"Hint: This job's run ('{run_name}') supports: {list(available_runtimes.keys()) or ['native']}",
                file=sys.stderr,
            )
            sys.exit(1)

    elif user_runtime_choice == "native":
        runtime_manifest_data["runtimeName"] = "native"

    elif not user_runtime_choice and available_runtimes:
        # User did not specify, but non-native runtimes are available. This is ambiguous.
        print(
            f"ERROR: Job '{job.id}' supports multiple runtimes: {list(available_runtimes.keys())}",
            file=sys.stderr,
        )
        print(
            "Hint: Please specify which to use with --runtime (e.g., --runtime podman, --runtime native)",
            file=sys.stderr,
        )
        sys.exit(1)

    # Fill in blank values for unused runtimes to satisfy the contract
    runtime_manifest_data.setdefault("ociTool", "")
    runtime_manifest_data.setdefault("ociImageArtifact", "")
    runtime_manifest_data.setdefault("bwrapSandboxArtifact", "")
    runtime_manifest_data.setdefault("bwrapCacheRoot", "")
    # <<< THE FIX IS HERE >>>
    # Provide the logical path to the lab root, satisfying the new contract key.
    runtime_manifest_data["labRoot"] = str(lab_path)
    print(f"[DEBUG-RUNNER] labRoot is set to {str(lab_path)}")

    # 3. Execute the job with the chosen runtime
    execute_job(job, lab_path, job_cache_dir, runtime_manifest_data)


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
    parser.add_argument(
        "--cache",
        type=Path,
        help="Directory for extracted runtime artifacts (e.g., sandboxes).",
    )
    parser.add_argument(
        "--runtime",
        default="native",
        type=str,
        help="Runtime tool for jobs (e.g., 'podman', 'bubblewrap', 'native').",
    )
    args = parser.parse_args()

    job_id = Path(args.job).name

    lab_path = args.lab

    job_cache_dir = (
        args.job_cache.resolve() if args.job_cache else find_writable_cache_dir()
    )

    artifact_cache_dir = None
    if args.cache:
        artifact_cache_dir = args.cache.resolve()
    elif args.runtime == "bubblewrap":
        artifact_cache_dir = job_cache_dir / "artifacts"
        print(
            f"[DEBUG-RUNNER] Auto-creating artifact cache at: {artifact_cache_dir}",
            file=sys.stderr,
        )

    try:
        exp = Experiment(lab_path)
    except FileNotFoundError as e:
        print(f"Error: {e}\nHint: Make sure the lab path is correct.", file=sys.stderr)
        sys.exit(1)

    target_job = exp.get_job(job_id)

    print(f"Starting debug run for job: {target_job.id}")
    job_cache_dir.mkdir(exist_ok=True)
    if artifact_cache_dir:
        artifact_cache_dir.mkdir(exist_ok=True)

    user_runtime_choice = args.runtime or None

    ensure_job_is_run(
        target_job,
        exp,
        lab_path,
        job_cache_dir,
        user_runtime_choice,
        artifact_cache_dir,
    )
    print(
        f"\n---"
        f"\nDebug run finished successfully."
        f"\nFinal output for job {target_job.id} is in: {job_cache_dir / target_job.id / 'out'}"
        f"\n---"
    )


if __name__ == "__main__":
    main()
