_: {
  pname = "simple-summer";

  inputs = {
    input_csv = "";
  };

  outputs = {
    "result.sum" = "$out/sum.txt";
  };

  run =
    { inputs, outputs, ... }:
    ''
      awk -F, 'NR > 1 { sum += $2 } END { print sum }' "${inputs.input_csv}" > "${outputs."result.sum"}"
    '';
}
