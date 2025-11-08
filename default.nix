{
  pkgs,
}:

pkgs.python3Packages.buildPythonPackage {
  pname = "repx-py";
  version = "0.1.0";

  src = ./src;

  pyproject = true;

  build-system = [
    pkgs.python3Packages.setuptools
  ];

  dependencies = [
    pkgs.python3Packages.pandas
  ];

  pythonImportsCheck = [
    "repx_py.models"
  ];

  meta = {
    description = "Library for analyzing RepX lab results";
  };
}
