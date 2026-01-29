"""Tests for edge cases and error handling in models."""

from pathlib import Path

import pytest
from repx_py.models import (
    Experiment,
    JobCollection,
    JobView,
    LocalCacheResolver,
    ManifestResolver,
)


class TestLocalCacheResolver:
    """Tests for LocalCacheResolver class."""

    def test_resolve_path_structure(self, tmp_path):
        resolver = LocalCacheResolver(tmp_path)

        class MockJob:
            id = "test-job-123"

        result = resolver.resolve_path(MockJob(), "output.csv")
        expected = tmp_path / "test-job-123" / "out" / "output.csv"
        assert result == expected

    def test_resolve_path_with_string_input(self, tmp_path):
        resolver = LocalCacheResolver(str(tmp_path))
        assert resolver.cache_dir == tmp_path.resolve()

    def test_default_cache_dir(self):
        resolver = LocalCacheResolver()
        assert resolver.cache_dir == Path(".repx-cache").resolve()

    def test_resolve_nested_path(self, tmp_path):
        resolver = LocalCacheResolver(tmp_path)

        class MockJob:
            id = "job-456"

        result = resolver.resolve_path(MockJob(), "nested/dir/file.txt")
        expected = tmp_path / "job-456" / "out" / "nested/dir/file.txt"
        assert result == expected


class TestManifestResolver:
    """Tests for ManifestResolver class."""

    def test_resolve_path_from_mapping(self):
        mapping = {
            "job-1": "/nix/store/abc-result",
            "job-2": Path("/nix/store/def-result"),
        }
        resolver = ManifestResolver(mapping)

        class MockJob:
            id = "job-1"

        result = resolver.resolve_path(MockJob(), "data.csv")
        assert result == Path("/nix/store/abc-result/data.csv")

    def test_resolve_path_not_found(self):
        resolver = ManifestResolver({"job-1": "/path"})

        class MockJob:
            id = "unknown-job"

        with pytest.raises(FileNotFoundError, match="No output path recorded"):
            resolver.resolve_path(MockJob(), "file.txt")

    def test_path_conversion_to_pathlib(self):
        mapping = {"job-1": "/some/string/path"}
        resolver = ManifestResolver(mapping)
        assert isinstance(resolver.mapping["job-1"], Path)


class TestJobCollection:
    """Tests for JobCollection edge cases."""

    def test_empty_collection(self, experiment):
        """Test behavior with empty collection."""
        empty_collection = experiment.jobs().filter(name="nonexistent-stage-xyz")
        assert len(empty_collection) == 0

    def test_iteration(self, experiment):
        """Test that JobCollection is iterable."""
        jobs = experiment.jobs()
        count = 0
        for job in jobs:
            assert isinstance(job, JobView)
            count += 1
        assert count == len(jobs)

    def test_slicing(self, experiment):
        """Test slicing returns a new JobCollection."""
        jobs = experiment.jobs()
        if len(jobs) >= 2:
            sliced = jobs[:2]
            assert isinstance(sliced, JobCollection)
            assert len(sliced) == 2

    def test_filter_with_unknown_operator(self, experiment):
        """Test filtering with an unknown operator falls back to equality."""
        jobs = experiment.jobs()
        filtered = jobs.filter(name__unknownop="value")
        assert len(filtered) == 0

    def test_filter_chain(self, experiment):
        """Test chaining multiple filters."""
        jobs = experiment.jobs()
        filtered = jobs.filter(stage_type="simple")
        further_filtered = filtered.filter(name__contains="producer")

        for job in further_filtered:
            assert job.stage_type == "simple"
            assert "producer" in job.name


class TestJobView:
    """Tests for JobView edge cases."""

    def test_repr(self, experiment):
        """Test the __repr__ method."""
        job = experiment.jobs()[0]
        repr_str = repr(job)
        assert "<JobView" in repr_str
        assert job.id in repr_str or job.name in repr_str

    def test_getattr_unknown_raises(self, experiment):
        """Test that accessing unknown attributes raises AttributeError."""
        job = experiment.jobs()[0]
        with pytest.raises(AttributeError, match="has no attribute"):
            _ = job.completely_unknown_attribute_xyz

    def test_get_output_path_key_not_found(self, experiment):
        """Test get_output_path with invalid key raises KeyError."""
        job = experiment.jobs()[0]
        with pytest.raises(KeyError, match="not found"):
            job.get_output_path("nonexistent_output_key")


class TestExperiment:
    """Tests for Experiment edge cases."""

    def test_experiment_requires_path_or_metadata(self):
        """Test that Experiment raises if neither path nor metadata provided."""
        with pytest.raises(
            ValueError, match="Either 'lab_path' or '_preloaded_metadata'"
        ):
            Experiment(lab_path=None)

    def test_runs_returns_dict(self, experiment):
        """Test that runs() returns a dictionary."""
        runs = experiment.runs()
        assert isinstance(runs, dict)
        assert len(runs) > 0

    def test_get_job_caching(self, experiment):
        """Test that get_job uses caching."""
        jobs = experiment.jobs()
        if len(jobs) > 0:
            job_id = jobs[0].id
            view1 = experiment.get_job(job_id)
            view2 = experiment.get_job(job_id)
            assert view1 is view2

    def test_get_job_not_found(self, experiment):
        """Test get_job with invalid ID raises KeyError."""
        with pytest.raises(KeyError, match="not found"):
            experiment.get_job("completely-invalid-job-id-that-does-not-exist")

    def test_get_run_for_job_not_found(self, experiment):
        """Test get_run_for_job with invalid ID raises KeyError."""
        with pytest.raises(KeyError, match="Could not find a run"):
            experiment.get_run_for_job("invalid-job-id-xyz")

    def test_jobs_returns_collection(self, experiment):
        """Test that jobs() returns a JobCollection."""
        jobs = experiment.jobs()
        assert isinstance(jobs, JobCollection)

    def test_effective_params_property(self, experiment):
        """Test effective_params property returns a dict."""
        params = experiment.effective_params
        assert isinstance(params, dict)


class TestExperimentFromRunMetadata:
    """Tests for Experiment.from_run_metadata factory method."""

    def test_metadata_file_not_found(self, tmp_path):
        """Test error when metadata file doesn't exist."""
        fake_path = tmp_path / "nonexistent.json"
        with pytest.raises(FileNotFoundError, match="Metadata file not found"):
            Experiment.from_run_metadata(fake_path, tmp_path)

    def test_invalid_metadata_type(self, tmp_path):
        """Test error when metadata has wrong type."""
        import json

        meta_file = tmp_path / "metadata.json"
        meta_file.write_text(json.dumps({"type": "not_a_run", "name": "test"}))

        with pytest.raises(ValueError, match="Expected metadata type 'run'"):
            Experiment.from_run_metadata(meta_file, tmp_path)

    def test_valid_metadata_creates_experiment(self, tmp_path):
        """Test that valid metadata creates an Experiment."""
        import json

        meta_file = tmp_path / "metadata.json"
        meta_file.write_text(
            json.dumps({"type": "run", "name": "test-run", "jobs": {}})
        )

        exp = Experiment.from_run_metadata(meta_file, tmp_path)
        assert "test-run" in exp.runs()


class TestJobCollectionToDataframe:
    """Tests for JobCollection.to_dataframe()."""

    def test_dataframe_columns(self, experiment):
        """Test that to_dataframe includes expected columns."""
        df = experiment.jobs().to_dataframe()
        assert "name" in df.columns or "name" in df.index.names

    def test_empty_collection_dataframe(self, experiment):
        """Test to_dataframe on empty collection returns empty dataframe."""
        import pandas as pd

        empty = experiment.jobs().filter(name="nonexistent")
        df = empty.to_dataframe()
        assert isinstance(df, pd.DataFrame)
        assert df.empty
