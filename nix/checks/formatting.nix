{ pkgs }:
let
  formatter = pkgs.callPackage ../formatter.nix { };
in
{
  fmt =
    pkgs.runCommand "check-formatting"
      {
        nativeBuildInputs = [ formatter ];
        src = ./../..;
      }
      ''
        cp -r $src ./src
        chmod -R +w ./src
        cd ./src

        echo "Running treefmt check..."
        treefmt --ci -v

        touch $out
      '';
}
