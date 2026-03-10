{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-non-nixos-remote-test.nix {
  inherit pkgs repx referenceLab;
  testName = "non-nixos-remote-docker-impure";
  runtime = "docker";
  mountMode = "impure";
  useSubset = true;
  bannerText = "NON-NIXOS REMOTE DOCKER IMPURE TEST COMPLETED";
}
