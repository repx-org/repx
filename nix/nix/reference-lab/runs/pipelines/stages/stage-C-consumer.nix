_: {
  pname = "stage-C-consumer";

  inputs = {
    data_a = "";
    list_b = "";
  };

  outputs = {
    "combined_list" = "$out/combined_list.txt";
  };

  run =
    { inputs, outputs, ... }:
    ''
      echo "Stage C: Concatenating A and B"
      cat "${inputs.data_a}" "${inputs.list_b}" > "${outputs."combined_list"}"
    '';
}
