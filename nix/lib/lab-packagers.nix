{
  pkgs,
  gitHash,
  lab_version,
}:
let
  repxVersion = "0.5.0";

  repx-expand = import ./internal/repx-expand-pkg.nix { inherit pkgs; };

  rsyncStatic =
    (pkgs.pkgsStatic.rsync.override {
      enableXXHash = false;
    }).overrideAttrs
      (_: {
        doCheck = false;
      });

  hostToolBinaries = [
    {
      pkg = pkgs.pkgsStatic.coreutils;
      bins = null;
    }
    {
      pkg = pkgs.pkgsStatic.jq;
      bins = [ "jq" ];
    }
    {
      pkg = pkgs.pkgsStatic.findutils;
      bins = [
        "find"
        "xargs"
      ];
    }
    {
      pkg = pkgs.pkgsStatic.gnused;
      bins = [ "sed" ];
    }
    {
      pkg = pkgs.pkgsStatic.gnugrep;
      bins = [ "grep" ];
    }
    {
      pkg = pkgs.pkgsStatic.bash;
      bins = [ "bash" ];
    }
    {
      pkg = pkgs.pkgsStatic.gnutar;
      bins = [ "tar" ];
    }
    {
      pkg = pkgs.pkgsStatic.pigz;
      bins = [
        {
          src = "pigz";
          dst = "gzip";
        }
      ];
    }
    {
      pkg = pkgs.pkgsStatic.bubblewrap;
      bins = [ "bwrap" ];
    }
    {
      pkg = pkgs.pkgsStatic.openssh;
      bins = null;
    }
    {
      pkg = rsyncStatic;
      bins = [ "rsync" ];
    }
  ];

  hostToolsHash = builtins.hashString "sha256" (
    pkgs.lib.concatMapStringsSep "-" (spec: builtins.baseNameOf (toString spec.pkg)) hostToolBinaries
  );

  common = import ./internal/common.nix;

  hostToolsJson = map (
    spec:
    let
      pkgPath = builtins.unsafeDiscardStringContext (toString spec.pkg);
      pkgHash = builtins.baseNameOf pkgPath;
    in
    {
      pkg_path = pkgPath;
      pkg_hash = pkgHash;
      bins =
        if spec.bins == null then
          null
        else
          map (binSpec: if builtins.isAttrs binSpec then binSpec else binSpec) spec.bins;
    }
  ) hostToolBinaries;

  blueprint2Lab =
    {
      runDefinitions,
      resolvedGroups ? { },
      containerMode ? "unified",
    }:
    let
      runs = runDefinitions;
      includeImages = containerMode != "none";

      allScriptDrvs = common.uniqueDrvs (pkgs.lib.concatMap (run: run.scriptDrvs or [ ]) runs);

      allImageContents = common.uniqueDrvs (
        pkgs.lib.flatten (pkgs.lib.map (run: run.imageContents or [ ]) runs)
      );

      unifiedImage =
        if containerMode == "unified" && allImageContents != [ ] then
          pkgs.dockerTools.buildLayeredImage {
            name = "repx-shared-image";
            tag = "latest";
            compressor = "none";
            contents = allImageContents;
            config = {
              Cmd = [ "${pkgs.bash}/bin/bash" ];
            };
          }
        else
          null;

      perRunImages =
        if containerMode == "per-run" then
          pkgs.lib.listToAttrs (
            pkgs.lib.filter (entry: entry.value != null) (
              map (
                run:
                let
                  contents = run.imageContents or [ ];
                in
                {
                  inherit (run) name;
                  value =
                    if contents != [ ] then
                      pkgs.dockerTools.buildLayeredImage {
                        name = run.name + "-image";
                        tag = "latest";
                        compressor = "none";
                        inherit contents;
                        config = {
                          Cmd = [ "${pkgs.bash}/bin/bash" ];
                        };
                      }
                    else
                      null;
                }
              ) runs
            )
          )
        else
          { };

      imageDerivations = common.uniqueDrvs (
        (pkgs.lib.optional (unifiedImage != null) unifiedImage) ++ (builtins.attrValues perRunImages)
      );

      runTemplates = map (
        run:
        run.runTemplate
        // {
          image_path =
            if containerMode == "per-run" && perRunImages ? "${run.name}" then
              let
                img = perRunImages.${run.name};
              in
              if img != null then
                "images/" + builtins.baseNameOf (builtins.unsafeDiscardStringContext (toString img))
              else
                null
            else
              null;
        }
      ) runs;

      blueprintData = {
        runs = runTemplates;
        host_tools = {
          hash = hostToolsHash;
          binaries = hostToolsJson;
        };
        git_hash = gitHash;
        repx_version = repxVersion;
        inherit lab_version;
        container_mode = containerMode;
        groups = resolvedGroups;
        unified_image_path =
          if unifiedImage != null then
            "images/" + builtins.baseNameOf (builtins.unsafeDiscardStringContext (toString unifiedImage))
          else
            null;
      };

      blueprintJson = builtins.unsafeDiscardStringContext (builtins.toJSON blueprintData);
      blueprintFile = pkgs.writeText "blueprint.json" blueprintJson;

      imageAssemblyScript = pkgs.lib.optionalString includeImages ''
        mkdir -p $out/images

        ${pkgs.lib.concatMapStringsSep "\n" (
          imageDrv:
          let
            imageBasename = builtins.baseNameOf (toString imageDrv);
          in
          ''
            image_tarball=$(${pkgs.findutils}/bin/find "${imageDrv}" -name "*.tar.gz" -o -name "*.tar" | head -n 1)
            if [ -z "$image_tarball" ]; then
              echo "Error: Could not find container image tarball in ${imageDrv}"; exit 1;
            fi

            image_dir="$out/images/${imageBasename}"
            mkdir -p "$image_dir"

            temp_extract=$(mktemp -d)
            ${pkgs.gnutar}/bin/tar -xf "$image_tarball" -C "$temp_extract"

            cp "$temp_extract/manifest.json" "$image_dir/manifest.json"

            for json_file in "$temp_extract"/*.json; do
              if [ -f "$json_file" ] && [ "$(basename "$json_file")" != "manifest.json" ]; then
                cp "$json_file" "$image_dir/"
              fi
            done

            layer_paths=$(${pkgs.jq}/bin/jq -r '.[0].Layers[]' "$image_dir/manifest.json")

            for layer_path in $layer_paths; do
              layer_hash=$(dirname "$layer_path")
              layer_store_name="''${layer_hash}-layer.tar"

              if [ ! -f "$out/store/$layer_store_name" ]; then
                cp "$temp_extract/$layer_path" "$out/store/$layer_store_name"
              fi

              mkdir -p "$image_dir/$layer_hash"
              ln -s "../../../store/$layer_store_name" "$image_dir/$layer_hash/layer.tar"
            done

            rm -rf "$temp_extract"
          ''
        ) imageDerivations}
      '';

    in
    {
      lab = pkgs.stdenv.mkDerivation {
        name = "hpc-experiment-lab";
        version = repxVersion;
        nativeBuildInputs = [
          repx-expand
          pkgs.jq
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnutar
        ]
        ++ allScriptDrvs;

        hostToolPaths = pkgs.lib.concatMapStringsSep " " (spec: toString spec.pkg) hostToolBinaries;

        buildCommand = ''
          for p in $hostToolPaths; do
            test -e "$p" || { echo "FATAL: host tool not found: $p"; exit 1; }
          done

          mkdir -p $out $out/lab $out/readme $out/store $out/jobs $out/revision $out/host-tools

          ${imageAssemblyScript}

          cat > $out/readme/README.md <<'REPX_README_EOF'
          This lab directory is a self-contained "seed" for your experiments.
          Built with repx ${repxVersion}. Assembled by repx-expand.
          REPX_README_EOF

          repx-expand \
            --blueprint ${blueprintFile} \
            --output $out \
            --lab-version "${lab_version}"

          echo "Lab built successfully."
        '';
      };
    };
in
{
  inherit blueprint2Lab;
}
