import logging
from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("repx-py")
except PackageNotFoundError:
    __version__ = "unknown"


from .models import (
    ArtifactResolver,
    Experiment,
    JobCollection,
    JobView,
    LocalCacheResolver,
    ManifestResolver,
)

logging.getLogger(__name__).addHandler(logging.NullHandler())

__all__ = [
    "ArtifactResolver",
    "Experiment",
    "JobCollection",
    "JobView",
    "LocalCacheResolver",
    "ManifestResolver",
    "__version__",
]
