import json
from pathlib import Path
from typing import (
    Any,
    Callable,
    Dict,
    Iterable,
    Iterator,
    List,
    Sequence,
    Set,
    Tuple,
    Union,
    overload,
)

import pandas as pd


class JobView:
    """
    A lightweight, read-only view of a single job's data.
    It combines the raw metadata with the calculated effective parameters.
    """

    def __init__(self, job_id: str, experiment: "Experiment"):
        self._id = job_id
        self._exp = experiment
        self._data = self._exp._get_complete_job_data(self._id)

    @property
    def id(self) -> str:
        return self._id

    @property
    def name(self) -> str:
        return self._data.get("name", self._id)

    @property
    def entrypoint_contract(self) -> List[str] | None:
        """The contract (list of required keys) for the job's entrypoint script manifest."""
        return self._data.get("entrypoint_contract")

    @property
    def params(self) -> Dict[str, Any]:
        """The user-defined parameters for this specific job."""
        return self._data.get("params", {})

    @property
    def effective_params(self) -> Dict[str, Any]:
        """The full, calculated effective parameters, including from dependencies."""
        return self._data.get("effective_params", {})

    def __getattr__(self, key: str) -> Any:
        if key in self._data:
            return self._data[key]
        raise AttributeError(f"'JobView' object has no attribute or data key '{key}'")

    def __repr__(self) -> str:
        return f"<JobView id={self.id}>"

    def load_csv(
        self, filename: str, cache_dir: str | Path = ".repx-cache", **kwargs
    ) -> pd.DataFrame:
        """Loads a CSV from a job's output directory in the debug cache."""
        output_path = Path(cache_dir).resolve() / self.id / "out" / filename
        return pd.read_csv(output_path, **kwargs)


class JobCollection(Sequence[JobView]):
    """
    Represents a collection of jobs that can be filtered fluently.
    Behaves like a read-only sequence of JobView objects.
    """

    def __init__(self, experiment: "Experiment", job_ids: Iterable[str]):
        self._exp = experiment
        self._job_ids = list(job_ids)

    def filter(self, predicate: Callable[[JobView], bool]) -> "JobCollection":
        """Filters the collection based on a predicate function."""
        filtered_ids = [
            job_id
            for job_id in self._job_ids
            if (job_view := self._exp.get_job(job_id)) and predicate(job_view)
        ]
        return JobCollection(self._exp, filtered_ids)

    def to_dataframe(self) -> pd.DataFrame:
        """Converts the effective parameters of all jobs in the collection to a pandas DataFrame."""
        records = [
            self._exp.get_job(job_id).effective_params for job_id in self._job_ids
        ]
        return pd.DataFrame.from_records(records, index=self._job_ids)

    def __iter__(self) -> Iterator[JobView]:
        for job_id in self._job_ids:
            yield self._exp.get_job(job_id)

    def __len__(self) -> int:
        return len(self._job_ids)

    @overload
    def __getitem__(self, index: int) -> JobView: ...

    @overload
    def __getitem__(self, index: slice) -> "JobCollection": ...

    def __getitem__(self, index: Union[int, slice]) -> Union[JobView, "JobCollection"]:
        if isinstance(index, slice):
            return JobCollection(self._exp, self._job_ids[index])
        return self._exp.get_job(self._job_ids[index])

    def __repr__(self) -> str:
        return f"<JobCollection size={len(self)}>"


class Experiment:
    """The main entry point for interacting with a RepX lab."""

    def __init__(self, lab_path: str | Path):
        self.path = Path(lab_path).resolve()

        potential_path = self.path / "metadata.json"
        if potential_path.is_file():
            self.metadata_path = potential_path
        else:
            revision_dir = self.path / "revision"
            if not revision_dir.is_dir():
                raise FileNotFoundError(
                    f"Could not find metadata.json or a 'revision' directory in {self.path}"
                )
            found = list(revision_dir.glob("**/metadata.json"))
            if not found:
                raise FileNotFoundError(
                    f"Could not find metadata.json in {revision_dir}"
                )
            self.metadata_path = found[0]

        with open(self.metadata_path, "r") as f:
            self._metadata = json.load(f)

        self._job_view_cache: Dict[str, JobView] = {}
        self._effective_params_cache = self._calculate_all_effective_params()

        self._job_to_run_map: Dict[str, str] = {}
        for run_name, run_data in self.runs().items():
            for job_id in run_data.get("jobs", []):
                self._job_to_run_map[job_id] = run_name

    def _get_single_effective_params(
        self, job_id: str, all_jobs_data: Dict, visiting: Set[str], memo: Dict
    ) -> Dict[str, Any]:
        if job_id in memo:
            return memo[job_id]
        if job_id in visiting:
            raise RecursionError(f"Circular dependency detected at: {job_id}")

        visiting.add(job_id)
        job_data = all_jobs_data.get(job_id)
        if not job_data:
            raise KeyError(f"Job ID '{job_id}' not found in metadata.")

        effective_params: Dict[str, Any] = {}
        for dep_mapping in job_data.get("input_mappings", []):
            if dep_id := dep_mapping.get("job_id"):
                effective_params.update(
                    self._get_single_effective_params(
                        dep_id, all_jobs_data, visiting, memo
                    )
                )

        effective_params.update(job_data.get("params", {}))
        visiting.remove(job_id)
        memo[job_id] = effective_params
        return effective_params

    def _calculate_all_effective_params(self) -> Dict[str, Dict]:
        all_jobs_data = self._metadata.get("jobs", {})
        if not all_jobs_data:
            return {}

        final_results = {}
        memo: Dict[str, Dict] = {}
        for job_id in all_jobs_data:
            final_results[job_id] = self._get_single_effective_params(
                job_id, all_jobs_data, set(), memo
            )
        return final_results

    @property
    def effective_params(self) -> Dict[str, Dict]:
        """A dictionary mapping all job IDs to their calculated effective parameters."""
        return self._effective_params_cache

    def _get_complete_job_data(self, job_id: str) -> Dict[str, Any]:
        """Combines raw job metadata with its calculated effective parameters."""
        raw_data = self._metadata.get("jobs", {}).get(job_id, {})
        effective_params = self._effective_params_cache.get(job_id, {})
        complete_data = raw_data.copy()
        complete_data["effective_params"] = effective_params
        return complete_data

    def get_job(self, job_id: str) -> JobView:
        """Retrieves a single job by its ID."""
        if job_id not in self._job_view_cache:
            if job_id not in self._metadata.get("jobs", {}):
                raise KeyError(f"Job ID '{job_id}' not found.")
            self._job_view_cache[job_id] = JobView(job_id, self)
        return self._job_view_cache[job_id]

    def get_run_for_job(self, job_id: str) -> Tuple[str, Dict[str, Any]]:
        """Finds the run definition that a given job belongs to."""
        run_name = self._job_to_run_map.get(job_id)
        if not run_name:
            raise KeyError(f"Could not find a run containing job '{job_id}'.")
        return run_name, self.runs()[run_name]

    def jobs(self) -> JobCollection:
        """Returns a filterable collection of all jobs in the experiment."""
        return JobCollection(self, self._metadata.get("jobs", {}).keys())

    def runs(self) -> Dict[str, Any]:
        """Returns the raw 'runs' dictionary from the metadata."""
        return self._metadata.get("runs", {})
