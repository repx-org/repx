{ pkgs }:
{
  pname = "stage-D-partial-sums";

  scatter = {
    pname = "scatter";
    inputs = {
      number_list_file = "";
    };
    outputs = {
      worker__arg = {
        startIndex = 0;
      };
      "work__items" = "$out/work_items.json";
    };
    runDependencies = with pkgs; [
      coreutils
      jq
    ];
    run =
      { inputs, outputs, ... }:
      ''
        LIST_FILE="${inputs.number_list_file}"
        NUM_LINES=$(wc -l < "$LIST_FILE")
        jq -n --argjson count "$NUM_LINES" '[range($count) | { "startIndex": .}]' > "${outputs.work__items}"
      '';
  };

  worker = {
    pname = "worker";
    inputs = {
      worker__item = "";
      number_list_file = "";
    };
    outputs = {
      "partial_sum" = "$out/worker-result.txt";
    };
    runDependencies = with pkgs; [
      coreutils
      jq
      gawk
    ];
    run =
      { inputs, outputs, ... }:
      ''
        WORK_ITEM_FILE="${inputs.worker__item}"
        START_INDEX=$(jq -r '.startIndex' "$WORK_ITEM_FILE")
        LIST_FILE="${inputs.number_list_file}"
        PARTIAL_SUM=$(tail -n +$((START_INDEX + 1)) "$LIST_FILE" | awk '{s+=$1} END {print s}')
        echo "$PARTIAL_SUM" > "${outputs.partial_sum}"
      '';
  };

  gather = {
    pname = "gather";
    inputs = {
      "worker__outs" = "[]";
    };
    outputs = {
      "data__partial_sums" = "$out/partial_sums.txt";
    };
    runDependencies = with pkgs; [
      coreutils
      jq
    ];
    run =
      { inputs, outputs, ... }:
      ''
        jq -r '.[].partial_sum' "${inputs.worker__outs}" | xargs cat > "${outputs."data__partial_sums"}"
      '';
  };
}
