{ pkgs }:
stageDef:
let
  pname = stageDef.pname;
  version = stageDef.version or "1.1";
  inputsDef = stageDef.inputs or { };
  outputsDef = stageDef.outputs or { };
  paramsDef = stageDef.paramInputs or { };
  dependencyDerivations = stageDef.dependencyDerivations or [ ];
  runDependencies = stageDef.runDependencies or [ ];

  bashInputs = pkgs.lib.mapAttrs (name: _: "\${inputs[\"${name}\"]}") inputsDef;
  bashOutputs = outputsDef;
  bashParams = pkgs.lib.mapAttrs (name: value: pkgs.lib.escapeShellArg value) paramsDef;

  userScript = stageDef.run {
    inputs = bashInputs;
    outputs = bashOutputs;
    params = bashParams;
    inherit pkgs;
  };

  binPath = pkgs.lib.makeBinPath runDependencies;

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

    # Dynamically declare bash variables from the inputs JSON
    ${pkgs.lib.concatMapStringsSep "\n" (name: ''
      ${name}="''${inputs["${name}"]:-}"
    '') (pkgs.lib.attrNames inputsDef)}

    # We check json_content first to avoid issues with `set -u` and empty associative arrays.
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

  depders = pkgs.lib.filter pkgs.lib.isDerivation dependencyDerivations;
  dependencyManifestJson = builtins.toJSON (map (drv: toString drv) depders);
in
pkgs.stdenv.mkDerivation rec {
  inherit pname version;
  dontUnpack = true;

  phases = [
    "checkPhase"
    "installPhase"
  ];

  passthru = (stageDef.passthru or { }) // {
    paramInputs = paramsDef;
    repxStageType = "simple";
    executables = {
      main = {
        inputs = stageDef.inputMappings or [ ];
        outputs = outputsDef;
      };
    };
    outputMetadata = outputsDef;
    stageInputs = stageDef.stageInputs or { };
  };

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin
    cp ${fullScript} $out/bin/${pname}
    chmod +x $out/bin/${pname}
    echo '${builtins.toJSON paramsDef}' > $out/${pname}-params.json
    echo '${dependencyManifestJson}' > $out/nix-input-dependencies.json
    runHook postInstall
  '';

  buildInputs = depders ++ runDependencies;

  nativeBuildInputs = [ pkgs.shellcheck ];
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    echo "--- Running shellcheck on the generated script for [${pname}] ---"
    # To ignore a specific error (e.g., SC2086), you can add '-e SC2086'.
    # Example: shellcheck -e SC2086 ${fullScript}
    shellcheck -W 0 ${fullScript}
    echo "--- Shellcheck passed for [${pname}] ---"
    runHook postCheck
  '';
}
