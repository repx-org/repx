{ pkgs }:
{
  pname = "simple-summer";

  inputs = {
    input_csv = "";
  };

  outputs = {
    "result.sum" = "$out/sum.txt";
  };

  runDependencies = [ pkgs.gawk ];

  run =
    { inputs, outputs, ... }:
    ''
      # Skip header, sum the second column (comma separated)
      awk -F, 'NR > 1 { sum += $2 } END { print sum }' "${inputs.input_csv}" > "${outputs."result.sum"}"
    '';
}
