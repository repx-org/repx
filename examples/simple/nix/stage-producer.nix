{ pkgs }:
{
  pname = "simple-producer";

  outputs = {
    "data.csv" = "$out/data.csv";
  };

  runDependencies = [ pkgs.python3 ];

  run =
    { outputs, ... }:
    ''
      python3 -c "
      import math
      with open('${outputs."data.csv"}', 'w') as f:
          f.write('x,y\n')
          for i in range(100):
              x = i / 10.0
              y = math.sin(x) + 2  # +2 to keep it positive for summation visual
              f.write(f'{x},{y}\n')
      "
    '';
}
