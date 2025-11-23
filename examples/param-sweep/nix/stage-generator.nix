{ pkgs }:
{
  pname = "generator";

  outputs = {
    "data.csv" = "$out/data.csv";
  };

  params = {
    slope = 1;
  };

  runDependencies = [ pkgs.python3 ];

  run =
    { outputs, params, ... }:
    ''
      echo "Running generator with slope: ${params.slope}"

      python3 -c "
      with open('${outputs."data.csv"}', 'w') as f:
          f.write('x,y\n')
          slope = float(${params.slope})
          for x in range(20):
              y = slope * (x ** 2)
              f.write(f'{x},{y}\n')
      "
    '';
}
