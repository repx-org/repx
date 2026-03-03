"""
Shared helper for NixOS tests: find a small subset of jobs from a lab manifest
to avoid running the full job suite (800+ jobs) during integration tests.
"""

from get_subset_jobs.core import get_subset_jobs

__all__ = ["get_subset_jobs"]
