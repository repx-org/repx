{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-e2e-local-podman";
  runtime = "podman";
  mountMode = "default";
  extraValidation = ''machine.succeed(f"grep -rE '540|595' {base_path}/outputs/*/out/total_sum.txt")'';
  bannerText = "E2E LOCAL PODMAN TEST COMPLETED";
}
