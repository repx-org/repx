"""Core logic for finding a representative subset of jobs."""

import json
import os


def get_subset_jobs(
    lab_path,
    run_name="simulation-run",
    prefer_name="workload-generator",
    prefer_resource_hints=False,
):
    """Walk the lab directory to find a representative subset of jobs.

    Returns a list of job IDs (typically one) suitable for integration testing.
    """
    print(f"Searching for jobs in {lab_path}")
    for root, _dirs, files in os.walk(lab_path):
        for file in files:
            if not file.endswith(".json"):
                continue
            full_path = os.path.join(root, file)
            try:
                with open(full_path) as f:
                    data = json.load(f)
                    if data.get("name") != run_name or "jobs" not in data:
                        continue
                    jobs = data["jobs"]

                    if prefer_resource_hints:
                        for jid, jval in jobs.items():
                            if jval.get("resource_hints"):
                                print(f"Found job with resource_hints: {jid}")
                                return [jid]

                    for jid, jval in jobs.items():
                        if prefer_name in jval.get("name", ""):
                            print(f"Found {prefer_name} job: {jid}")
                            return [jid]

                    if jobs:
                        first_job = next(iter(jobs.keys()))
                        print(
                            f"Preferred job not found. Selecting first available: {first_job}"
                        )
                        return [first_job]
            except Exception as e:
                print(f"Warning: Failed to read or parse {full_path}: {e}")
    return []
