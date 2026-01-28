{ pkgs, ... }:
{
  pname = "make-stage";
  version = "1.0";

  outputs = {
    "run.log" = "$out/run.log";
  };

  runDependencies = [
    pkgs.make-pkg
  ];

  run =
    { outputs, ... }:
    ''
      echo "Running binary from package..."

      make-pkg > "${outputs."run.log"}"

      echo "Execution successful."
    '';
}
