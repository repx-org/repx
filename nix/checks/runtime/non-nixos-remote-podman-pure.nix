{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-non-nixos-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "non-nixos-remote-podman-pure";
  runtime = "podman";
  mountMode = "pure";
  useSubset = true;
  bannerText = "NON-NIXOS REMOTE PODMAN PURE TEST COMPLETED";
}
