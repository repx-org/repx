{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-non-nixos-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "non-nixos-remote-podman-impure";
  runtime = "podman";
  mountMode = "impure";
  useSubset = true;
  bannerText = "NON-NIXOS REMOTE PODMAN IMPURE TEST COMPLETED";
}
