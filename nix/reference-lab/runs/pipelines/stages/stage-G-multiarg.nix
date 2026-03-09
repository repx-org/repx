_: {
  pname = "stage-G-multiarg";

  parameters = {
    workload_args = "";
    offset = 0;
  };

  inputs = {
    source_data = "";
  };

  resources = {
    mem = "256M";
    cpus = 1;
    time = "00:02:00";
  };

  outputs = {
    "data.multiarg_result" = "$out/multiarg_result.txt";
  };

  run =
    {
      inputs,
      outputs,
      parameters,
      ...
    }:
    ''
      echo "Stage G: Demonstrating multi-arg parameter expansion"
      read -ra ARGS <<< "${parameters.workload_args}"

      echo "Received ''${#ARGS[@]} arguments:"
      for i in "''${!ARGS[@]}"; do
        echo "  arg[$i] = ''${ARGS[$i]}"
      done

      offset="${parameters.offset}"

      result=0
      for arg in "''${ARGS[@]}"; do
        result=$((result + arg + offset))
      done

      if [[ -f "${inputs.source_data}" ]]; then
        upstream=$(head -1 "${inputs.source_data}")
        result=$((result + upstream))
        echo "Added upstream value: $upstream"
      fi

      echo "Sum of args (with offset=$offset): $result"
      echo "$result" > "${outputs."data.multiarg_result"}"
    '';
}
