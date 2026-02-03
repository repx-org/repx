{
  pkgs,
  flake-utils,
  repx,
  docs,
}:
{
  default = flake-utils.lib.mkApp {
    drv = repx;
    name = "repx";
  };

  check-repx-examples = flake-utils.lib.mkApp {
    drv = pkgs.callPackage ./apps/check-repx-examples.nix {
      inherit repx;
    };
  };

  docs-preview = flake-utils.lib.mkApp {
    drv = pkgs.writeShellScriptBin "docs-preview" ''
      echo "Building documentation..."
      ${docs}

      echo -e "\n\033[1;32mServing documentation at http://localhost:8080/\033[0m"

      cd ${docs}
      ${pkgs.python3}/bin/python3 -m http.server 8080
    '';
  };
}
