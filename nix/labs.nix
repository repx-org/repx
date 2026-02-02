{
  pkgs,
  repx-lib,
  gitHash,
}:

{
  reference-lab =
    (pkgs.callPackage ./reference-lab/lab.nix {
      inherit pkgs repx-lib;
      inherit gitHash;
    }).lab;

  simple-lab =
    (pkgs.callPackage ../examples/simple/nix/lab.nix {
      inherit pkgs repx-lib;
      gitHash = "docs-gen";
    }).lab;

  sweep-lab =
    (pkgs.callPackage ../examples/param-sweep/nix/lab.nix {
      inherit pkgs repx-lib;
      gitHash = "docs-gen";
    }).lab;
}
