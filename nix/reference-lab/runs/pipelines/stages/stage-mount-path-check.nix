_: {
  pname = "mount-path-check";

  parameters = {
    check_path = "";
  };

  resources = {
    mem = "256M";
    cpus = 1;
    time = "00:02:00";
  };

  outputs = {
    "mount_check_result" = "$out/mount_check_result.txt";
  };

  run =
    { outputs, parameters, ... }:
    ''
      CHECK_PATH="${parameters.check_path}"
      echo "Checking if mount path is accessible: $CHECK_PATH"

      if [ -f "$CHECK_PATH" ]; then
        echo "MOUNT_PATH_ACCESSIBLE"
        echo "Content: $(cat "$CHECK_PATH")"
        cat "$CHECK_PATH" > "${outputs."mount_check_result"}"
      else
        echo "MOUNT_PATH_NOT_FOUND: $CHECK_PATH"
        echo "MOUNT_PATH_NOT_FOUND" > "${outputs."mount_check_result"}"
      fi
    '';
}
