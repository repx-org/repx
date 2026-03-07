_: {
  pname = "stage-A-producer";
  parameters = {
    offset = 0;
    template_dir = "";
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

      offset="${parameters.offset}"
      for i in {1..5}; do
        echo $((i + offset))
      done > "${outputs."data_a"}"
    '';
}
