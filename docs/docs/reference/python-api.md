# Python API Reference

The `repx-py` library provides a high-level interface for querying experiment results and loading artifacts. Install it with:

```bash
pip install repx-py
```

## `repx_py.Experiment`

The main entry point for interacting with a RepX Lab.

### Constructor

```python
Experiment(
    lab_path: str | Path | None = None,
    resolver: ArtifactResolver | None = None,
)
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `lab_path` | `str \| Path \| None` | `None` | Path to the built Lab directory (usually `./result`). |
| `resolver` | `ArtifactResolver \| None` | `None` | Strategy for locating job output files. Defaults to `LocalCacheResolver()`. |

At least one of `lab_path` or an internal `_preloaded_metadata` dict must be provided.

**Basic usage:**

```python
from repx_py import Experiment

exp = Experiment(lab_path="./result")
```

### Factory Methods

#### `Experiment.from_run_metadata(metadata_path, store_base)`

Create an `Experiment` from a single run's metadata file. Useful inside an analysis stage where only upstream metadata and a base store path are available.

```python
@classmethod
def from_run_metadata(
    cls,
    metadata_path: str | Path,
    store_base: str | Path,
) -> Experiment
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `metadata_path` | `str \| Path` | Path to a `metadata__*.json` file provided as an input. |
| `store_base` | `str \| Path` | Root path where job outputs are stored (usually `store__base`). |

**Example:**

```python
exp = Experiment.from_run_metadata(
    metadata_path=inputs["metadata__simulation"],
    store_base=inputs["store__base"],
)
```

### Methods

#### `runs() -> dict[str, Any]`

Returns a dictionary mapping run names to their raw metadata dicts. Each dict contains keys like `"name"`, `"type"`, `"jobs"`, and `"params"`.

```python
for run_name, run_data in exp.runs().items():
    print(run_name, len(run_data["jobs"]))
```

#### `jobs() -> JobCollection`

Returns a `JobCollection` containing all jobs across all runs.

```python
all_jobs = exp.jobs()
print(f"Total jobs: {len(all_jobs)}")
```

#### `get_job(job_id: str) -> JobView`

Retrieve a single job by its hash ID. Raises `KeyError` if the job is not found.

```python
job = exp.get_job("abc123def456")
```

#### `get_run_for_job(job_id: str) -> tuple[str, dict]`

Returns `(run_name, run_data)` for the run that contains the given job. Raises `KeyError` if the job is not found in any run.

```python
run_name, run_data = exp.get_run_for_job("abc123def456")
print(f"Job belongs to run: {run_name}")
```

### Properties

#### `effective_params -> dict[str, dict]`

A dictionary mapping every job ID to its **effective** (inherited) parameters. Effective parameters include all parameters inherited from upstream stages via the dependency graph.

```python
for job_id, params in exp.effective_params.items():
    print(job_id, params)
```

---

## `repx_py.JobCollection`

A queryable, list-like collection of jobs. Implements `Sequence[JobView]` -- supports iteration, `len()`, indexing, and slicing.

### Methods

#### `filter(predicate=None, **kwargs) -> JobCollection`

Filter jobs by parameter values, metadata attributes, or a callable predicate.

```python
def filter(
    self,
    predicate: Callable[[JobView], bool] | None = None,
    **kwargs,
) -> JobCollection
```

**Exact match filtering:**

```python
# Filter by exact parameter value
training_jobs = exp.jobs().filter(name="train")

# Multiple filters (AND logic)
subset = exp.jobs().filter(name="train", seed=42)
```

**Operator-based filtering** using `__` suffixes:

| Operator | Description | Example |
|----------|-------------|---------|
| `__startswith` | String prefix match | `name__startswith="preprocess"` |
| `__endswith` | String suffix match | `name__endswith="analysis"` |
| `__contains` | Substring match | `name__contains="train"` |

```python
# Filter by name prefix
preprocess_jobs = exp.jobs().filter(name__startswith="preprocess")

# Filter by substring
gpu_jobs = exp.jobs().filter(name__contains="gpu")
```

**Callable predicate filtering:**

```python
# Custom predicate function
large_jobs = exp.jobs().filter(lambda job: job.effective_params.get("size", 0) > 1000)
```

#### `to_dataframe() -> pandas.DataFrame`

Convert the collection to a Pandas DataFrame. Columns include `job_id` (set as index if unique), `name`, and all effective parameter keys (flattened via `json_normalize`).

```python
df = exp.jobs().to_dataframe()
print(df.head())
```

### Sequence Operations

```python
collection = exp.jobs()

# Length
len(collection)

# Iteration
for job in collection:
    print(job.name)

# Indexing
first_job = collection[0]

# Slicing (returns a new JobCollection)
first_ten = collection[:10]
```

---

## `repx_py.JobView`

A read-only view of a single job's metadata, effective parameters, and output artifacts.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | `str` | The unique hash ID of the job. |
| `name` | `str` | The stage name. |
| `stage_type` | `str` | Either `"simple"` or `"scatter-gather"`. |
| `params` | `dict[str, Any]` | The job's own (direct) parameters. |
| `effective_params` | `dict[str, Any]` | Resolved parameters including those inherited from upstream stages. |
| `outputs` | `dict[str, str]` | Map of output names to path templates (e.g. `{"data": "$out/data.csv"}`). For scatter-gather stages, returns the gather outputs. |
| `input_mappings` | `list[dict[str, Any]]` | List of input mapping dicts describing where each input comes from. |
| `executable_path` | `str \| None` | Relative path to the job's main executable (simple stages only). |
| `dependencies` | `JobCollection` | A `JobCollection` of upstream jobs (derived from input mappings). |

**Attribute fallback:** Accessing any attribute not listed above falls through to the raw metadata dict. This lets you access any metadata field directly:

```python
job = exp.get_job("abc123")
print(job.stage_type)        # "simple"
print(job.effective_params)  # {"seed": 42, "model": "resnet"}
```

### Data Access Methods

#### `get_output_path(output_key: str) -> Path`

Get the resolved filesystem path to a named output. The `$out/` prefix is stripped and the path is resolved through the configured `ArtifactResolver`.

```python
path = job.get_output_path("data_csv")
print(path)  # /home/user/.repx-cache/<job-id>/out/data.csv
```

Raises `KeyError` if the output key is not found. Available keys are listed in `job.outputs`.

#### `load_csv(output_key_or_filename: str, **kwargs) -> pandas.DataFrame`

Load a CSV file into a Pandas DataFrame. Accepts either:
- An **output key** name (looked up via `outputs` and resolved through the artifact resolver)
- A **raw filename** (resolved directly through the artifact resolver as a relative path)

Extra `kwargs` are passed to `pd.read_csv`.

```python
# By output key
df = job.load_csv("results")

# By raw filename
df = job.load_csv("metrics.csv", sep="\t")
```

---

## Artifact Resolvers

Resolvers determine how `JobView` locates physical output files on disk. All resolvers implement the `ArtifactResolver` interface.

### `ArtifactResolver` (Abstract Base Class)

```python
class ArtifactResolver(ABC):
    @abstractmethod
    def resolve_path(self, job: JobView, relative_path: str) -> Path:
        """Resolve a path relative to the job's output directory."""
        ...
```

### `LocalCacheResolver`

The default resolver. Locates artifacts in a local `.repx-cache` directory structure.

```python
LocalCacheResolver(cache_dir: str | Path = ".repx-cache")
```

Resolution pattern: `<cache_dir>/<job_id>/out/<relative_path>`

```python
from repx_py import Experiment, LocalCacheResolver

exp = Experiment(
    lab_path="./result",
    resolver=LocalCacheResolver(cache_dir="/data/experiment-outputs"),
)
```

### `ManifestResolver`

Resolves artifacts via a pre-computed mapping from job IDs to output directories. Useful when outputs are in non-standard locations.

```python
ManifestResolver(job_output_map: dict[str, str | Path])
```

```python
from repx_py import Experiment, ManifestResolver

resolver = ManifestResolver({
    "abc123": "/scratch/run1/outputs/abc123",
    "def456": "/scratch/run1/outputs/def456",
})
exp = Experiment(lab_path="./result", resolver=resolver)
```

Raises `FileNotFoundError` if a job ID is not in the mapping.
