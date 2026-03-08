_: {
  pname = "hello";

  outputs = {
    "greeting.txt" = "$out/greeting.txt";
  };

  run =
    { outputs, ... }:
    ''
      echo "Hello from repx!" > "${outputs."greeting.txt"}"
    '';
}
