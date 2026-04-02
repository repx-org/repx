_: {
  pname = "stage-A-producer";
  parameters = {
    offset = 0;
    template_dir = "";
    nix_tool_bin = "";
  };

  resources = {
    mem = "256M";
    cpus = 1;
    time = "00:02:00";
  };

  outputs = {
    "data_a" = "$out/numbers.txt";
  };

  run =
    { outputs, parameters, ... }:
    ''
      echo "Stage A: Offset ${parameters.offset}, Template ${parameters.template_dir}"

      if [ -n "${parameters.nix_tool_bin}" ]; then
        "${parameters.nix_tool_bin}"/hello --version || { echo "ERROR: nix store tool not found in container"; exit 1; }
      fi

      offset="${parameters.offset}"
      for i in {1..5}; do
        echo $((i + offset))
      done > "${outputs."data_a"}"
    '';
}
