{
  pkgs,
  repx,
  referenceLab,
}:

import ./helpers/mk-runtime-test.nix {
  inherit pkgs repx referenceLab;
  testName = "repx-impure-mode-comprehensive";
  runtime = "bwrap";
  mountMode = "impure";
  extraValidation = ''machine.succeed(f"grep -rE '540|595' {base_path}/outputs/*/out/total_sum.txt")'';
  bannerText = "E2E IMPURE TEST COMPLETED";
}
