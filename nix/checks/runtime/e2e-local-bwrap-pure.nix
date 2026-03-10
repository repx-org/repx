{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-local-bwrap-pure";
  runtime = "bwrap";
  mountMode = "default";
  extraValidation = ''machine.succeed(f"grep -rE '540|595' {base_path}/outputs/*/out/total_sum.txt")'';
  bannerText = "E2E LOCAL BWRAP PURE TEST COMPLETED";
}
