{ pkgs }:
let
  extract = {
    pname = "extract";
    inputs = {
      worker__item = "";
      number_list_file = "";
    };
    outputs = {
      extracted_number = "$out/number.txt";
    };
    deps = [ ];
    runDependencies = with pkgs; [
      coreutils
      jq
      gawk
    ];
    resources = {
      mem = "512M";
      cpus = 1;
      time = "00:05:00";
    };
    run =
      { inputs, outputs, ... }:
      ''
        START_INDEX=$(jq -r '.startIndex' "${inputs.worker__item}")
        NUMBER=$(tail -n +$((START_INDEX + 1)) "${inputs.number_list_file}" | head -1)
        echo "$NUMBER" > "${outputs.extracted_number}"
      '';
  };

  square = {
    pname = "square";
    inputs = {
      extracted_number = "";
    };
    outputs = {
      squared = "$out/squared.txt";
    };
    deps = [ extract ];
    runDependencies = with pkgs; [
      coreutils
      gawk
    ];
    resources = {
      mem = "512M";
      cpus = 1;
      time = "00:05:00";
    };
    run =
      { inputs, outputs, ... }:
      ''
        N=$(cat "${inputs.extracted_number}")
        echo "$N * $N" | awk '{printf "%d\n", $1 * $3}' > "${outputs.squared}"
      '';
  };

  double = {
    pname = "double";
    inputs = {
      extracted_number = "";
    };
    outputs = {
      doubled = "$out/doubled.txt";
    };
    deps = [ extract ];
    runDependencies = with pkgs; [
      coreutils
      gawk
    ];
    resources = {
      mem = "512M";
      cpus = 1;
      time = "00:05:00";
    };
    run =
      { inputs, outputs, ... }:
      ''
        N=$(cat "${inputs.extracted_number}")
        echo "$((N * 2))" > "${outputs.doubled}"
      '';
  };

  combine = {
    pname = "combine";
    inputs = {
      squared = "";
      doubled = "";
    };
    outputs = {
      partial_sum = "$out/worker-result.txt";
    };
    deps = [
      square
      double
    ];
    runDependencies = with pkgs; [
      coreutils
      gawk
    ];
    resources = {
      mem = "512M";
      cpus = 1;
      time = "00:05:00";
    };
    run =
      { inputs, outputs, ... }:
      ''
        SQ=$(cat "${inputs.squared}")
        DB=$(cat "${inputs.doubled}")
        echo "$((SQ + DB))" > "${outputs.partial_sum}"
      '';
  };

in
{
  pname = "stage-D-partial-sums";

  resources = {
    mem = "256M";
    cpus = 1;
    time = "00:02:00";
  };

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

  steps = {
    inherit
      extract
      square
      double
      combine
      ;
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
    resources = {
      mem = "1G";
      cpus = 1;
      time = "00:10:00";
    };
    run =
      { inputs, outputs, ... }:
      ''
        jq -r '.[].partial_sum' "${inputs.worker__outs}" | xargs cat > "${outputs."data__partial_sums"}"
      '';
  };
}
