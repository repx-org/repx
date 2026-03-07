_: {
  pname = { parameters }: "stage-F-dynamic-${parameters.mode}";

  parameters = {
    mode = "default";
    multiplier = 1;
  };

  resources =
    { parameters }:
    {
      mem = if parameters.mode == "slow" then "1G" else "512M";
      cpus = if parameters.multiplier > 5 then 4 else 1;
      time = if parameters.mode == "slow" then "01:00:00" else "00:10:00";
    };

  inputs =
    { parameters }:
    {
      "source_data" = if parameters.mode != null then "" else "";
    };

  outputs =
    { parameters }:
    {
      result = "$out/result-${parameters.mode}.txt";
      summary = "$out/summary.txt";
    };

  run =
    {
      inputs,
      outputs,
      parameters,
      ...
    }:
    ''
      echo "Dynamic stage running in mode: ${parameters.mode}"
      echo "Multiplier: ${parameters.multiplier}"

      input_file="${inputs.source_data}"
      multiplier="${parameters.multiplier}"
      if [[ -f "$input_file" ]]; then
        value=$(cat "$input_file" | head -1)
        result=$((value * multiplier))
        echo "$result" > "${outputs.result}"
      else
        echo "0" > "${outputs.result}"
      fi

      echo "Processed with mode=${parameters.mode}, multiplier=${parameters.multiplier}" > "${outputs.summary}"
    '';
}
