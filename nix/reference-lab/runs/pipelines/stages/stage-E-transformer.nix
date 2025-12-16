{ pkgs }:
{
  pname = "stage-E-total-sum";

  inputs = {
    data__partial_sums = "";
  };

  outputs = {
    "data.total_sum" = "$out/total_sum.txt";
  };

  runDependencies = with pkgs; [ gawk ];

  run =
    { inputs, outputs, ... }:
    ''
      TOTAL_SUM=$(awk '{s+=$1} END {print s}' "${inputs.data__partial_sums}")
      echo "$TOTAL_SUM" > "${outputs."data.total_sum"}"
    '';
}
