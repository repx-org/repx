{ pkgs }:
{
  lint =
    pkgs.runCommand "deadnix"
      {
        nativeBuildInputs = [
          pkgs.deadnix
        ];
        src = ./../..;
      }
      ''
        cp -r $src ./src
        chmod -R +w ./src
        cd ./src
        echo "Running Deadnix..."
        deadnix --fail .
        touch $out
      '';
}
