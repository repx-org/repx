_: {
  pname = "stage-B-producer";

  params = {
    mode = "default";
    config_file = "";
  };

  outputs = {
    "raw_output" = "$out/numbers.txt";
  };

  run =
    { outputs, params, ... }:
    ''
      echo "Stage B: Mode ${params.mode}, Config ${params.config_file}"

      printf "6\n7\n8\n9\n10\n" > "${outputs."raw_output"}"
    '';
}
