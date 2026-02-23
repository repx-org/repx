{ pkgs }:
{
  pname,
  version ? "1.1",
  paramInputs ? { },
  userScript,
  runDependencies ? [ ],
}:
let
  common = import ./common.nix;
  baseContainerPkgs = common.mkRuntimePackages pkgs;

  binPath = pkgs.lib.makeBinPath (baseContainerPkgs ++ runDependencies);

  header = ''
    export PATH="${binPath}"${if binPath == "" then "" else ":"}$PATH
    set -euxo pipefail
    export out="$1"
    export inputs_json="$2"

    declare -A inputs
    json_content=""
    if [[ -f "$inputs_json" ]]; then
        json_content=$(cat "$inputs_json")
        while read -r key value; do
            inputs["$key"]="$value"
        done < <(echo "$json_content" | ${pkgs.jq}/bin/jq -r 'to_entries[] | .key + " " + .value')
    fi

    if [[ -n "$json_content" ]] && [[ "$json_content" != "{}" ]]; then
      echo "Verifying all stage inputs are ready..." >&2
      TIMEOUT_SECONDS=30
      SLEEP_INTERVAL=2
      elapsed=0
      while [ $elapsed -lt $TIMEOUT_SECONDS ]; do
        all_inputs_ready=true
        for input_path in "''${inputs[@]}"; do
          if ! { [ -f "$input_path" ] || [ -d "$input_path" ]; } || [ ! -r "$input_path" ]; then
            all_inputs_ready=false
            echo "  - Waiting for: $input_path" >&2
            break
          fi
        done
        if [ "$all_inputs_ready" = true ]; then
          echo "All inputs are ready. Proceeding with stage execution." >&2
          break
        fi
        sleep $SLEEP_INTERVAL
        elapsed=$((elapsed + SLEEP_INTERVAL))
        if [ $elapsed -ge $TIMEOUT_SECONDS ]; then
            echo "ERROR: Timed out after $TIMEOUT_SECONDS seconds waiting for inputs to become available." >&2
            exit 1
        fi
      done
    fi

    mkdir -p "$out"
    echo "Clearing output directory for a clean run: $out" >&2
    find "$out" -mindepth 1 -not -name 'slurm-*.out' -delete
    mkdir -p "$out"
    cd "$out"
  '';

  fullScript = pkgs.writeScript "${pname}-script" ''
    #!${pkgs.bash}/bin/bash
    ${header}
    ${userScript}
  '';

  paramsJson = builtins.toJSON paramInputs;
  analyzerScript = ./analyze_deps.py;

  shellBuiltins = [
    "cd"
    "echo"
    "printf"
    "read"
    "set"
    "unset"
    "export"
    "declare"
    "typeset"
    "local"
    "eval"
    "source"
    "."
    "test"
    "true"
    "false"
    "exit"
    "return"
    "wait"
    "trap"
    "exec"
    "shift"
    "command"
    "type"
    "hash"
    "alias"
    "unalias"
    "mapfile"
    "readarray"
  ];
in
pkgs.stdenv.mkDerivation {
  pname = "${pname}-script";
  inherit version;
  dontUnpack = true;

  phases = [
    "checkPhase"
    "installPhase"
  ];

  inherit paramsJson;
  passAsFile = [ "paramsJson" ];

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin
    cp ${fullScript} $out/bin/${pname}
    chmod +x $out/bin/${pname}
    cp "$paramsJsonPath" $out/${pname}-params.json
    runHook postInstall
  '';

  buildInputs = runDependencies;

  nativeBuildInputs = [
    pkgs.shellcheck
    pkgs.oils-for-unix
    (pkgs.python3.withPackages (ps: [ ps.bashlex ]))
  ]
  ++ baseContainerPkgs;

  doCheck = true;

  ALLOWED_BUILTINS = builtins.concatStringsSep " " shellBuiltins;

  checkPhase = ''
    runHook preCheck
    echo "--- Running checks for [${pname}] ---"

    echo "Running shellcheck..."
    shellcheck -W 0 ${fullScript}

    echo "--- [DEBUG] Running Dependency Checks ---"

    osh -n --ast-format text ${fullScript} > script.ast

    python3 ${analyzerScript} script.ast --json dependency_report.json

    echo "--- Checks finished ---"
    runHook postCheck
  '';

  passthru = {
    scriptPath = fullScript;
  };
}
