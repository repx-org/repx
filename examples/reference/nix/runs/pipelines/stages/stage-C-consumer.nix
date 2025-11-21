{ pkgs }:
{
  pname = "stage-C-consumer";

  inputs = {
    list_a = ""; # From A
    list_b = ""; # From B
  };

  outputs = {
    "data.combined_list" = "$out/combined_list.txt";
  };

  run =
    { inputs, outputs, ... }:
    ''
      echo "Stage C: Concatenating lists from A and B"
      cat "${inputs.list_a}" "${inputs.list_b}" > "${outputs."data.combined_list"}"
      echo "Combined list created."
    '';
}
