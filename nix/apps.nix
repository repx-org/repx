{
  pkgs,
  flake-utils,
  repx,
  docs,
}:
{
  default =
    flake-utils.lib.mkApp {
      drv = repx;
      name = "repx";
    }
    // {
      meta.description = "The RepX runner binary";
    };

  docs-preview =
    flake-utils.lib.mkApp {
      drv = pkgs.writeShellScriptBin "docs-preview" ''
        echo -e "\033[1;32mServing documentation at http://localhost:8080/\033[0m"
        cd ${docs}
        ${pkgs.python3}/bin/python3 -m http.server 8080
      '';
    }
    // {
      meta.description = "Preview documentation locally";
    };
}
