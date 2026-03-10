{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "e2e-local-bwrap-impure";
  runtime = "bwrap";
  mountMode = "impure";
  extraValidation = ''machine.succeed(f"grep -rE '540|595' {base_path}/outputs/*/out/total_sum.txt")'';
  bannerText = "E2E LOCAL BWRAP IMPURE TEST COMPLETED";
}
