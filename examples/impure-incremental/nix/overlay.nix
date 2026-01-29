{
  common = final: _prev: {
    run-impure-lab = final.callPackage ./pkgs/run-impure-lab.nix { };
  };

  pure = final: _prev: {
    make-pkg = final.callPackage ./pkgs/pure-make-pkg.nix { };
  };

  impure = final: _prev: {
    make-pkg = final.callPackage ./pkgs/impure-make-pkg.nix { };
  };
}
