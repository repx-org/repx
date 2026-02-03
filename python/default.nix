{
  pkgs,
  reference-lab ? null,
}:

pkgs.python3Packages.buildPythonPackage {
  pname = "repx-py";
  inherit ((pkgs.lib.importTOML ./src/pyproject.toml).project) version;

  src = ./src;

  pyproject = true;

  build-system = [
    pkgs.python3Packages.setuptools
  ];

  nativeBuildInputs = [
    pkgs.makeWrapper
  ];

  propagatedBuildInputs = with pkgs.python3Packages; [
    pandas
  ];

  makeWrapperArgs = [ ];

  nativeCheckInputs = [
    pkgs.python3Packages.pytest
    pkgs.python3Packages.pytest-mock
  ];

  doCheck = reference-lab != null;

  checkPhase = ''
    export REFERENCE_LAB_PATH=${if reference-lab != null then reference-lab else ""}

    export PYTHONPATH=$PWD:$PYTHONPATH

    pytest tests
  '';

  pythonImportsCheck = [
    "repx_py.models"
  ];

  meta = {
    description = "Library for analyzing RepX lab results";
  };
}
