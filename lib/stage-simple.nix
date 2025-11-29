{ pkgs }:
stageDef:
let
  inherit (stageDef) pname;
  version = stageDef.version or "1.1";
  inputsDef = stageDef.inputs or { };
  outputsDef = stageDef.outputs or { };
  paramsDef = stageDef.paramInputs or { };
  dependencyDerivations = stageDef.dependencyDerivations or [ ];
  runDependencies = stageDef.runDependencies or [ ];

  bashInputs = pkgs.lib.mapAttrs (name: _: "\${inputs[\"${name}\"]}") inputsDef;
  bashOutputs = outputsDef;
  bashParams = pkgs.lib.mapAttrs (_: value: pkgs.lib.escapeShellArg value) paramsDef;
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
  dependencyManifestJson = builtins.toJSON (map toString depders);

  baseContainerPkgs = with pkgs; [
    bash
    coreutils
    findutils
    gnused
    gawk
    gnugrep
    jq
  ];

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

  nativeBuildInputs = [
    pkgs.shellcheck
    pkgs.oils-for-unix
    pkgs.python3
  ]
  ++ baseContainerPkgs;

  doCheck = true;

  ALLOWED_BUILTINS = builtins.concatStringsSep " " shellBuiltins;

  checkPhase = ''
    runHook preCheck
    echo "--- Running checks for [${pname}] ---"

    echo "Running shellcheck..."
    shellcheck -W 0 ${fullScript}

    echo "Running OSH dependency analysis..."

    osh -n --ast-format text ${fullScript} > script.ast

    python3 ${pkgs.writeText "verify_deps.py" ''
      import sys, re, shutil, os, ast

      allowed_builtins = set(os.environ.get("ALLOWED_BUILTINS", "").split())

      try:
          with open("script.ast", "r") as f:
              ast_text = f.read()
      except FileNotFoundError:
          print("Error: Could not read script.ast")
          sys.exit(1)

      pattern = re.compile(r'blame_tok:\(Token id:[\w_]+ length:(\d+) col:(\d+) line:\(SourceLine .*? content:(".*?")\s+src:', re.DOTALL)

      commands = set()
      for match in pattern.finditer(ast_text):
          try:
              length = int(match.group(1))
              col = int(match.group(2))
              line_content = ast.literal_eval(match.group(3))

              if col < len(line_content):
                  cmd = line_content[col : col + length]
                  commands.add(cmd)
          except Exception as e:
              continue

      errors = []
      for cmd in commands:
          if not cmd: continue
          if cmd.startswith("/") or cmd.startswith("."): continue # Absolute paths ok
          if cmd.startswith("$"): continue # Variables ok (dynamic)
          if cmd in allowed_builtins: continue

          if shutil.which(cmd): continue

          errors.append(cmd)

      if errors:
          print("\n" + "="*60)
          print(f"dependency-check ERROR in [${pname}]")
          print("The following commands are used but not found in the container or dependencies:")
          for e in errors:
              print(f"  - {e}")
          print("\nSolution: Add these packages to 'runDependencies' in your stage definition.")
          print("="*60 + "\n")
          sys.exit(1)
      else:
          print("Dependency check passed.")
    ''}

    echo "--- All checks passed for [${pname}] ---"
    runHook postCheck
  '';
}
