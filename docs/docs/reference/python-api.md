# Python API Reference

The `repx-py` library provides a high-level interface for querying experiment results and loading artifacts.

## `repx_py.Experiment`

The entry point for interacting with a RepX Lab.

```python
from repx_py import Experiment
exp = Experiment(lab_path="./result")
```

### Constructor

*   `Experiment(lab_path: str | Path)`
    *   Initialize with the path to the built Lab directory (usually `./result`).

### Properties

*   `runs` (Dict[str, RunView]): Access runs by name.
*   `jobs` (JobCollection): A collection of *all* jobs across all runs.

## `repx_py.RunView`

Represents a single named run (e.g., "simulation").

### Properties

*   `name` (str): The run name.
*   `jobs` (JobCollection): A collection of jobs belonging to this run.

## `repx_py.JobCollection`

A queryable collection of jobs. It behaves like a list but provides filtering capabilities.

### Methods

*   `filter(**kwargs) -> JobCollection`
    *   Filter jobs by parameter values or metadata.
    *   Example: `exp.jobs.filter(model="resnet", learning_rate=0.01)`
*   `to_dataframe() -> pandas.DataFrame`
    *   Returns a DataFrame containing metadata (status, parameters, paths) for all jobs in the collection.
*   `__iter__()`
    *   Iterate over `JobView` objects.
*   `__len__()`
    *   Count of jobs.

## `repx_py.JobView`

Represents a single job execution (a concrete instance of a Stage).

### Properties

*   `id` (str): The unique hash ID of the job.
*   `name` (str): The stage name.
*   `pname` (str): The full unique name (including scatter indices).
*   `status` (str): Current status (COMPLETED, FAILED, etc.).
*   `effective_params` (dict): The resolved parameters used for this job.

### Data Access Methods

*   `get_output_path(output_name: str) -> Path`
    *   Get the absolute path to a specific output file.
*   `load_csv(output_name: str, **kwargs) -> pandas.DataFrame`
    *   Load an output CSV file directly into Pandas. `kwargs` are passed to `pd.read_csv`.
*   `load_json(output_name: str) -> dict`
    *   Load an output JSON file.
*   `load_text(output_name: str) -> str`
    *   Read a text file.
