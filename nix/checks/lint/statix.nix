{ pkgs }:
{
  lint =
    pkgs.runCommand "statix"
      {
        nativeBuildInputs = [
          pkgs.statix
        ];
        src = ./../..;
      }
      ''
        cp -r $src ./src
        chmod -R +w ./src
        cd ./src
        echo "Running Statix..."
        statix check .
        touch $out
      '';
}
