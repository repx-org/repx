import pandas as pd

from repx_py.models import (
    Experiment,
    JobCollection,
    JobView,
    LocalCacheResolver,
    ManifestResolver,
)


def test_experiment_loading(experiment: Experiment):
    """Tests that the Experiment object loads without errors."""
    assert experiment is not None
    assert "simulation-run" in experiment.runs()
    assert "analysis-run" in experiment.runs()


def test_job_collection_basics(experiment: Experiment):
    """Tests the JobCollection object and basic filtering."""
    jobs = experiment.jobs()
    assert isinstance(jobs, JobCollection)
    assert len(jobs) > 0

    consumer_jobs = jobs.filter(lambda j: j.name.startswith("stage-C-consumer"))
    assert len(consumer_jobs) == 1
    assert isinstance(consumer_jobs[0], JobView)


def test_job_collection_fluent_filter(experiment: Experiment):
    """Tests the new fluent kwarg-based filtering."""
    jobs = experiment.jobs()

    scatter_jobs = jobs.filter(stage_type="scatter-gather")
    assert len(scatter_jobs) > 0
    for job in scatter_jobs:
        assert job.stage_type == "scatter-gather"

    producer_jobs = jobs.filter(name__startswith="stage-A")
    assert len(producer_jobs) == 1
    assert producer_jobs[0].name.startswith("stage-A-producer")


def test_get_job_and_dependencies(experiment: Experiment):
    """Tests retrieving a single job and traversing dependencies."""
    consumer_job = experiment.jobs().filter(name__startswith="stage-C-consumer")[0]

    job = experiment.get_job(consumer_job.id)
    assert job.id == consumer_job.id

    deps = job.dependencies
    assert len(deps) == 2
    dep_names = sorted([d.name for d in deps])

    assert len(dep_names) == 2
    assert dep_names[0].startswith("stage-A-producer")
    assert dep_names[1].startswith("stage-B-producer")


def test_job_view_properties(experiment: Experiment):
    """Tests that the properties of a JobView are correctly populated."""
    total_sum_job = experiment.jobs().filter(name__startswith="stage-E-total-sum")[0]

    assert total_sum_job.params == {}
    assert total_sum_job.effective_params == {}
    assert total_sum_job.executable_path.endswith("/bin/stage-E-total-sum")
    assert len(total_sum_job.input_mappings) > 0
    assert total_sum_job.outputs["data.total_sum"] == "$out/total_sum.txt"


def test_job_collection_to_dataframe(experiment: Experiment):
    """Tests conversion of a JobCollection to a pandas DataFrame."""
    df = experiment.jobs().to_dataframe()
    assert isinstance(df, pd.DataFrame)
    assert not df.empty
    assert "name" in df.columns
    assert "job_id" in df.columns or df.index.name == "job_id"


def test_resolvers(experiment: Experiment, tmp_path):
    """Tests that resolvers function logically (mocking the files)."""
    job = experiment.jobs().filter(name__startswith="stage-A-producer")[0]

    cache_dir = tmp_path / "my-cache"
    resolver = LocalCacheResolver(cache_dir)
    resolved_path = resolver.resolve_path(job, "output.txt")
    expected_path = cache_dir / job.id / "out" / "output.txt"
    assert resolved_path == expected_path

    mapping = {job.id: "/nix/store/some-hash-result"}
    manifest_resolver = ManifestResolver(mapping)
    resolved_manifest_path = manifest_resolver.resolve_path(job, "output.txt")
    assert str(resolved_manifest_path) == "/nix/store/some-hash-result/output.txt"

    exp_manifest = Experiment(experiment.path, resolver=manifest_resolver)
    job_view = exp_manifest.get_job(job.id)

    final_path = job_view.get_output_path("data.numbers")
    assert str(final_path) == "/nix/store/some-hash-result/numbers.txt"
