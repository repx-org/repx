{ pkgs }:
let
  formatter = pkgs.callPackage ../formatters.nix { };
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

        custom-formatter

        touch $out
      '';
}
