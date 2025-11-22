{ pkgs }:
{
  lint =
    pkgs.runCommand "lint-code-base"
      {
        nativeBuildInputs = [
          pkgs.statix
          pkgs.deadnix
        ];
        src = ./..;
      }
      ''
        cp -r $src ./src
        chmod -R +w ./src
        cd ./src
        echo "Running Statix..."
        statix check .
        echo "Running Deadnix..."
        deadnix --fail .
        touch $out
      '';
}
