{
  pkgs,
  gitHash,
  lab_version,
}:
let
  repxVersion = "0.3.0";

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

  buildLabCoreAndManifest =
    {
      runs,
      includeImages,
      resolvedGroups ? { },
    }:
    let
      lib-run-internal = {
        run2Jobs =
          runDefinition:
          let
            pipelinesForRun = runDefinition.runs;
            nestedJobs = pkgs.lib.map (pipeline: pkgs.lib.attrValues pipeline) pipelinesForRun;
            allStageResults = pkgs.lib.flatten nestedJobs;
            allJobDerivations = pkgs.lib.filter pkgs.lib.isDerivation allStageResults;
          in
          pkgs.lib.unique allJobDerivations;
      };

      allJobDerivations = pkgs.lib.unique (
        pkgs.lib.flatten (pkgs.lib.map (run: lib-run-internal.run2Jobs run) runs)
      );

      imageDerivations = pkgs.lib.unique (
        pkgs.lib.filter (i: i != null) (pkgs.lib.map (run: run.image) runs)
      );

      metadataHelpers = (import ./metadata.nix) {
        inherit
          pkgs
          gitHash
          repxVersion
          includeImages
          ;
      };

      metadataDrvs =
        let
          accumulateMetadata =
            acc: runDef:
            let
              resolvedDependencies = pkgs.lib.listToAttrs (
                pkgs.lib.mapAttrsToList (
                  depName: depType:
                  let
                    depDrv =
                      acc.${depName}
                        or (throw "Dependency '${depName}' not found for run '${runDef.name}'. This should not happen if runs are sorted.");
                    depFilename = builtins.baseNameOf (toString depDrv);
                    relPath = "revision/${depFilename}";
                  in
                  {
                    name = builtins.unsafeDiscardStringContext relPath;
                    value = depType;
                  }
                ) (runDef.interRunDepTypes or { })
              );

              metadataDrv = metadataHelpers.mkRunMetadata {
                inherit runDef resolvedDependencies;
                jobs = lib-run-internal.run2Jobs runDef;
              };
            in
            acc
            // {
              "${runDef.name}" = metadataDrv;
            };
        in
        pkgs.lib.foldl' accumulateMetadata { } runs;

      rootMetadata = metadataHelpers.mkRootMetadata {
        runMetadataPaths = map (
          runDef:
          let
            drv = metadataDrvs.${runDef.name};
            filename = builtins.baseNameOf (toString drv);
          in
          "revision/${filename}"
        ) runs;
        groups = resolvedGroups;
      };
      rootMetadataFilename = builtins.baseNameOf (toString rootMetadata);

      labCoreBuildScript = ''
        mkdir -p $out/store $out/revision $out/jobs $out/host-tools/${hostToolsHash}/bin

        ${pkgs.lib.concatMapStringsSep "\n" (
          toolSpec:
          let
            inherit (toolSpec) pkg;
            pkgHash = builtins.baseNameOf (toString pkg);
          in
          if toolSpec.bins == null then
            ''
              for bin in ${pkg}/bin/*; do
                binname=$(basename "$bin")
                storename="${pkgHash}-$binname"
                if [ ! -f "$out/store/$storename" ]; then
                  cp "$bin" "$out/store/$storename"
                fi
                ln -sf "../../../store/$storename" "$out/host-tools/${hostToolsHash}/bin/$binname"
              done
            ''
          else
            pkgs.lib.concatMapStringsSep "\n" (
              binSpec:
              let
                srcName = if builtins.isAttrs binSpec then binSpec.src else binSpec;
                dstName = if builtins.isAttrs binSpec then binSpec.dst else binSpec;
                storeName = "${pkgHash}-${srcName}";
              in
              ''
                if [ ! -f "$out/store/${storeName}" ]; then
                  cp ${pkg}/bin/${srcName} "$out/store/${storeName}"
                fi
                ln -sf "../../../store/${storeName}" "$out/host-tools/${hostToolsHash}/bin/${dstName}"
              ''
            ) toolSpec.bins
        ) hostToolBinaries}

        ${pkgs.lib.concatMapStringsSep "\n" (
          jobDrv:
          let
            jobBasename = builtins.baseNameOf (toString jobDrv);
          in
          ''
            cp -rL ${jobDrv} $out/jobs/${jobBasename}
          ''
        ) allJobDerivations}

        cp ${rootMetadata} "$out/revision/${rootMetadataFilename}"

        ${pkgs.lib.concatMapStringsSep "\n" (drv: ''
          cp ${drv} "$out/revision/$(basename ${drv})"
        '') (pkgs.lib.attrValues metadataDrvs)}

        ${pkgs.lib.optionalString includeImages ''
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
        ''}
      '';

      labCore = pkgs.stdenv.mkDerivation {
        name = "hpc-lab-core";
        version = repxVersion;

        nativeBuildInputs = [
          pkgs.jq
        ];

        inherit labCoreBuildScript;
        passAsFile = [ "labCoreBuildScript" ];

        buildCommand = ''
          source "$labCoreBuildScriptPath"
        '';
      };

      labId = pkgs.lib.head (pkgs.lib.splitString "-" (builtins.baseNameOf (toString labCore)));
      labManifest = pkgs.writeText "lab-metadata.json" (
        builtins.toJSON {
          inherit labId;
          metadata = "revision/${rootMetadataFilename}";
        }
      );

      allReadmeParts = (import ./readme.nix) {
        inherit pkgs;
        jobDerivations = allJobDerivations;
      };

    in
    {
      inherit
        labCore
        labManifest
        allReadmeParts
        allJobDerivations
        ;
    };
  runs2Lab =
    {
      runDefinitions,
      resolvedGroups ? { },
    }:
    let
      runs = runDefinitions;
      artifacts = buildLabCoreAndManifest {
        inherit runs resolvedGroups;
        includeImages = true;
      };
      readme = pkgs.runCommand "README.md" { } ''
        cat ${artifacts.allReadmeParts.readmeNative}/README.md \
            ${artifacts.allReadmeParts.readmeContainer}/README_container.md > $out
      '';
    in
    {
      lab = pkgs.stdenv.mkDerivation {
        name = "hpc-experiment-lab";
        version = repxVersion;
        nativeBuildInputs = [
          artifacts.labCore
          pkgs.jq
          pkgs.coreutils
          pkgs.findutils
        ];

        passthru = {
          inherit (artifacts) allJobDerivations;
        };

        buildCommand = ''
          mkdir -p $out $out/lab $out/readme
          cp -r --no-dereference ${artifacts.labCore}/* $out/
          cp ${readme} $out/readme/$(basename ${readme})

          files_json_file=$(mktemp)
          find $out -type f | sort | while read -r filepath; do
            relpath="''${filepath#$out/}"
            hash=$(sha256sum "$filepath" | cut -d' ' -f1)
            printf '{"path":"%s","sha256":"%s"}\n' "$relpath" "$hash"
          done | jq -s '.' > "$files_json_file"

          labId=$(jq -r '.labId' ${artifacts.labManifest})
          metadata=$(jq -r '.metadata' ${artifacts.labManifest})

          jq -n \
            --arg labId "$labId" \
            --arg lab_version "${lab_version}" \
            --arg metadata "$metadata" \
            --slurpfile files "$files_json_file" \
            '{labId: $labId, lab_version: $lab_version, metadata: $metadata, files: $files[0]}' \
            > $out/lab/$(basename ${artifacts.labManifest})

          rm -f "$files_json_file"
          echo "Lab directory created successfully."
        '';
      };
    };
in
{
  inherit runs2Lab;
}
