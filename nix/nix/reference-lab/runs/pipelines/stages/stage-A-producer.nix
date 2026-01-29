_: {
  pname = "stage-A-producer";
  params = {
    offset = 0;
    template_dir = "";
  };

  outputs = {
    "data_a" = "$out/numbers.txt";
  };

  run =
    { outputs, params, ... }:
    ''
      echo "Stage A: Offset ${toString params.offset}, Template ${params.template_dir}"

      # Generate numbers 1-5 + offset
      for i in {1..5}; do
        echo $((i + ${toString params.offset}))
      done > "${outputs."data_a"}"
    '';
}
