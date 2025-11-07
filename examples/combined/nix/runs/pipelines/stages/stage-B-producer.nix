{ pkgs }:
{
  pname = "stage-B-producer";

  outputs = {
    "data.numbers" = "$out/numbers.txt";
  };

  run =
    { outputs, ... }:
    ''
      echo "Stage B: Producing number list 6-10"
      printf "6\n7\n8\n9\n10\n" > "${outputs."data.numbers"}"
    '';
}
