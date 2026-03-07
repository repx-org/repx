{ pkgs }:
{
  pname = "generator";

  outputs = {
    "data.csv" = "$out/data.csv";
  };

  parameters = {
    slope = 1;
  };

  runDependencies = [ pkgs.python3 ];

  run =
    { outputs, parameters, ... }:
    ''
      echo "Running generator with slope: ${parameters.slope}"

      python3 -c "
      with open('${outputs."data.csv"}', 'w') as f:
          f.write('x,y\n')
          slope = float(${parameters.slope})
          for x in range(20):
              y = slope * (x ** 2)
              f.write(f'{x},{y}\n')
      "
    '';
}
