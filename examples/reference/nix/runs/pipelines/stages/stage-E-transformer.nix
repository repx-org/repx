{ pkgs }:
{
  pname = "stage-E-total-sum";

  inputs = {
    data__partial_sums = "";
  };

  outputs = {
    "data.total_sum" = "$out/total_sum.txt";
  };

  run =
    { inputs, outputs, ... }:
    ''
      PARTIAL_SUMS_FILE="${inputs.data__partial_sums}"
      echo "Stage E: Calculating total sum from file $PARTIAL_SUMS_FILE"

      TOTAL_SUM=$(awk '{s+=$1} END {print s}' "$PARTIAL_SUMS_FILE")

      echo "Final total sum is: $TOTAL_SUM"
      echo "$TOTAL_SUM" > "${outputs."data.total_sum"}"
    '';
}
