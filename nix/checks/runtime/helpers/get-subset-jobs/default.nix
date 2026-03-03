{ buildPythonPackage, setuptools }:

buildPythonPackage {
  pname = "get-subset-jobs";
  version = "0.1.0";
  src = ./.;
  format = "setuptools";
  build-system = [ setuptools ];
}
