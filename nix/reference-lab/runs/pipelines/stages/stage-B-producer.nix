_: {
  pname = "stage-B-producer";

  parameters = {
    mode = "default";
    config_file = "";
  };

  resources = {
    mem = "512M";
    cpus = 1;
    time = "00:05:00";
  };

  outputs = {
    "raw_output" = "$out/numbers.txt";
  };

  run =
    { outputs, parameters, ... }:
    ''
      echo "Stage B: Mode ${parameters.mode}, Config ${parameters.config_file}"

      printf "6\n7\n8\n9\n10\n" > "${outputs."raw_output"}"
    '';
}
