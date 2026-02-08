_: {
  pname = { params }: "stage-F-dynamic-${params.mode}";

  params = {
    mode = "default";
    multiplier = 1;
  };

  inputs =
    { params }:
    {
      "source_data" = if params.mode != null then "" else "";
    };

  outputs =
    { params }:
    {
      result = "$out/result-${params.mode}.txt";
      summary = "$out/summary.txt";
    };

  run =
    {
      inputs,
      outputs,
      params,
      ...
    }:
    ''
      echo "Dynamic stage running in mode: ${params.mode}"
      echo "Multiplier: ${params.multiplier}"

      input_file="${inputs.source_data}"
      if [[ -f "$input_file" ]]; then
        value=$(cat "$input_file" | head -1)
        result=$((value * ${params.multiplier}))
        echo "$result" > "${outputs.result}"
      else
        echo "0" > "${outputs.result}"
      fi

      echo "Processed with mode=${params.mode}, multiplier=${params.multiplier}" > "${outputs.summary}"
    '';
}
