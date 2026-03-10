{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-non-nixos-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "non-nixos-remote-docker-pure";
  runtime = "docker";
  mountMode = "pure";
  useSubset = true;
  bannerText = "NON-NIXOS REMOTE DOCKER PURE TEST COMPLETED";
}
