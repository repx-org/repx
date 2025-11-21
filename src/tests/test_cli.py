import json
import subprocess
import sys
from pathlib import Path


from repx_py.models import Experiment


def test_cli_trace_params(lab_path: Path):
    """Tests the 'trace-params' CLI command."""
    command = [
        sys.executable,
        "-m",
        "repx_py.cli.trace_params",
        str(lab_path),
    ]

    result = subprocess.run(command, capture_output=True, text=True, check=True)

    assert result.returncode == 0

    params_data = json.loads(result.stdout)
    assert isinstance(params_data, dict)
    assert len(params_data) > 0

    expected_substring = "stage-C-consumer"
    found = any(expected_substring in k for k in params_data.keys())

    if not found:
        print(
            f"DEBUG: Expected '{expected_substring}' in keys, but found keys: {list(params_data.keys())}",
            file=sys.stderr,
        )

    assert found


def test_cli_debug_runner_simple_stage(lab_path: Path, tmp_path: Path):
    """
    Performs an end-to-end test of the 'debug-runner' by running a simple
    stage with dependencies and verifying its output.
    """
    exp = Experiment(lab_path)
    jobs = exp.jobs()

    target_job = next(
        (j for j in jobs if j.name.startswith("stage-C-consumer")),
        None,
    )
    assert target_job is not None, "Could not find stage-C-consumer in the lab"
    consumer_job_id = target_job.id

    cache_dir = tmp_path / "repx-cache"

    command = [
        sys.executable,
        "-m",
        "repx_py.cli.debug_runner",
        "--lab",
        str(lab_path),
        "--job",
        consumer_job_id,
        "--job-cache",
        str(cache_dir),
    ]

    result = subprocess.run(command, capture_output=True, text=True, check=False)

    print("--- STDOUT ---")
    print(result.stdout)
    print("--- STDERR ---")
    print(result.stderr)

    assert result.returncode == 0, "Debug runner failed to execute."

    output_file = cache_dir / consumer_job_id / "out" / "combined_list.txt"
    assert output_file.exists(), "Expected output file was not created."

    content = output_file.read_text().strip().split("\n")
    expected_content = [str(i) for i in range(1, 11)]
    assert content == expected_content, (
        f"Expected output {expected_content}, but got '{content}'."
    )
