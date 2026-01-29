{ pkgs }:
{
  check =
    pkgs.runCommand "check-shebangs"
      {
        src = ./../..;
      }
      ''
        cd $src

        if grep -rE '^#!/(bin/|usr/bin/)bash([[:space:]]|$)' . --exclude-dir=.git; then
          echo "ERROR: Absolute path bash shebang found. Use #!/usr/bin/env bash"
          exit 1
        fi

        if grep -rE '^#!/(bin/|usr/bin/)python[3]?([[:space:]]|$)' . --exclude-dir=.git; then
          echo "ERROR: Absolute path python shebang found. Use #!/usr/bin/env python3"
          exit 1
        fi

        touch $out
      '';
}
