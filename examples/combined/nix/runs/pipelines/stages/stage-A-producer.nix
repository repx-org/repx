{ pkgs }:
{
  pname = "stage-A-producer";

  outputs = {
    "data.numbers" = "$out/numbers.txt";
  };

  run =
    { outputs, ... }:
    ''
      echo "Stage A: Producing number list 1-5"
      printf "1\n2\n3\n4\n5\n" > "${outputs."data.numbers"}"
    '';
}
