{
  common = final: prev: {
    run-impure-lab = final.callPackage ./pkgs/run-impure-lab.nix { };
  };

  pure = final: prev: {
    make-pkg = final.callPackage ./pkgs/pure-make-pkg.nix { };
  };

  impure = final: prev: {
    make-pkg = final.callPackage ./pkgs/impure-make-pkg.nix { };
  };
}
