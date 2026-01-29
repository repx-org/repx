import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent.parent))

from repx_py.models import Experiment


@pytest.fixture(scope="session")
def lab_path() -> Path:
    """
    Provides the path to the reference lab, skipping tests if the path is not set.
    This path is passed from the Nix `checkPhase` via an environment variable.
    """
    path_str = os.environ.get("REFERENCE_LAB_PATH")
    if not path_str:
        pytest.skip(
            "REFERENCE_LAB_PATH environment variable not set. Skipping integration tests."
        )

    path = Path(path_str)
    if not path.exists():
        pytest.fail(f"Reference lab path does not exist: {path}")

    return path


@pytest.fixture(scope="session")
def experiment(lab_path: Path) -> Experiment:
    """
    Provides a loaded Experiment object for the entire test session,
    avoiding the need to reload it for every test.
    """
    return Experiment(lab_path)
