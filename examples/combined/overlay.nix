self: super: {
  myhello = super.callPackage ./nix/pkgs/myhello.nix { };
}
