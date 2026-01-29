"""Tests for the visualize module's utility functions."""

from repx_py.visualize import (
    PALETTE,
    clean_id,
    get_fill_color,
    get_varying_params,
    smart_truncate,
)


class TestGetFillColor:
    """Tests for the get_fill_color function."""

    def test_producer_returns_producer_color(self):
        assert get_fill_color("stage-producer-abc") == PALETTE["producer"]

    def test_consumer_returns_consumer_color(self):
        assert get_fill_color("stage-consumer-xyz") == PALETTE["consumer"]

    def test_worker_returns_worker_color(self):
        assert get_fill_color("data-worker-123") == PALETTE["worker"]

    def test_partial_returns_partial_color(self):
        assert get_fill_color("partial-sum-stage") == PALETTE["partial"]

    def test_total_returns_total_color(self):
        assert get_fill_color("total-sum-stage") == PALETTE["total"]

    def test_unknown_returns_default_color(self):
        assert get_fill_color("random-stage-name") == PALETTE["default"]

    def test_case_insensitive(self):
        assert get_fill_color("STAGE-PRODUCER") == PALETTE["producer"]
        assert get_fill_color("Stage-Consumer") == PALETTE["consumer"]

    def test_empty_string_returns_default(self):
        assert get_fill_color("") == PALETTE["default"]


class TestSmartTruncate:
    """Tests for the smart_truncate function."""

    def test_short_string_unchanged(self):
        assert smart_truncate("short", max_len=30) == "short"

    def test_long_string_truncated(self):
        long_str = "a" * 50
        result = smart_truncate(long_str, max_len=20)
        assert len(result) <= 20
        assert ".." in result

    def test_path_extracts_filename(self):
        result = smart_truncate("/very/long/path/to/filename.txt")
        assert result == "filename.txt"

    def test_brackets_removed(self):
        result = smart_truncate("[1, 2, 3]")
        assert "[" not in result
        assert "]" not in result

    def test_quotes_removed(self):
        result = smart_truncate("'quoted'")
        assert "'" not in result
        result = smart_truncate('"double"')
        assert '"' not in result

    def test_exact_max_len_unchanged(self):
        s = "x" * 10
        result = smart_truncate(s, max_len=10)
        assert result == s

    def test_non_string_converted(self):
        result = smart_truncate(12345)
        assert result == "12345"

    def test_max_len_boundary(self):
        s = "a" * 11
        result = smart_truncate(s, max_len=10)
        assert len(result) <= 10


class TestCleanId:
    """Tests for the clean_id function."""

    def test_removes_special_characters(self):
        assert clean_id("stage-A-producer") == "stageAproducer"
        assert clean_id("job@123#test") == "job123test"

    def test_keeps_alphanumeric_and_underscore(self):
        assert clean_id("valid_name_123") == "valid_name_123"

    def test_empty_string(self):
        assert clean_id("") == ""

    def test_only_special_chars(self):
        assert clean_id("@#$%^&*") == ""

    def test_unicode_removed(self):
        assert clean_id("name") == "name"


class MockJobView:
    """A simple mock for JobView used in tests."""

    def __init__(self, params: dict):
        self.params = params


class TestGetVaryingParams:
    """Tests for the get_varying_params function."""

    def test_empty_list_returns_empty_dict(self):
        assert get_varying_params([]) == {}

    def test_single_job_returns_all_params(self):
        mock_job = MockJobView({"count": 10, "name": "test"})
        result = get_varying_params([mock_job])
        assert "count" in result
        assert "name" in result

    def test_multiple_jobs_same_params_still_included(self):
        job1 = MockJobView({"x": 1})
        job2 = MockJobView({"x": 1})

        result = get_varying_params([job1, job2])
        assert "x" in result

    def test_varying_values_captured(self):
        job1 = MockJobView({"x": 1})
        job2 = MockJobView({"x": 2})

        result = get_varying_params([job1, job2])
        assert sorted(result["x"]) == [1, 2]

    def test_list_param_serialized(self):
        job1 = MockJobView({"items": [1, 2, 3]})
        result = get_varying_params([job1])
        assert "items" in result

    def test_dict_param_serialized(self):
        job1 = MockJobView({"config": {"key": "value"}})
        result = get_varying_params([job1])
        assert "config" in result

    def test_missing_param_excluded_from_values(self):
        job1 = MockJobView({"x": 1})
        job2 = MockJobView({})

        result = get_varying_params([job1, job2])
        if "x" in result:
            assert "?" not in result["x"]
