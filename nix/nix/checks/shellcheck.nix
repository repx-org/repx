{ pkgs }:
{
  lint =
    pkgs.runCommand "shellcheck"
      {
        nativeBuildInputs = [ pkgs.shellcheck ];
        src = ./../..;
      }
      ''
        cp -r $src ./src
        cd ./src

        find . -type f -not -path '*/.git/*' -exec sh -c '
          for file do
            if [[ "$file" == *.sh ]] || [[ "$file" == *.bash ]]; then
              echo "$file"
            elif head -n1 "$file" 2>/dev/null | grep -Eq "^#!/usr/bin/env[[:space:]]+bash$"; then
              echo "$file"
            fi
          done
        ' sh {} + | xargs -r shellcheck

        touch $out
      '';
}
