{
  pkgs,
  gitHash,
}:
let
  labVersion = "0.1.3";

  mkHostTools =
    let
      rsyncStatic =
        (pkgs.pkgsStatic.rsync.override {
          enableXXHash = false;
        }).overrideAttrs
          (_: {
            doCheck = false;
          });
    in
    pkgs.runCommand "host-tools"
      {
        buildInputs = [
          pkgs.pkgsStatic.coreutils
          pkgs.pkgsStatic.jq
          pkgs.pkgsStatic.findutils
          pkgs.pkgsStatic.gnused
          pkgs.pkgsStatic.gnugrep
          pkgs.pkgsStatic.bash
          pkgs.pkgsStatic.gnutar
          pkgs.pkgsStatic.pigz
          pkgs.pkgsStatic.bubblewrap
          rsyncStatic
          pkgs.pkgsStatic.openssh
        ];
      }
      ''
        mkdir -p $out/bin
        cp ${pkgs.pkgsStatic.coreutils}/bin/* $out/bin/
        cp ${pkgs.pkgsStatic.jq}/bin/jq $out/bin/
        cp ${pkgs.pkgsStatic.findutils}/bin/find $out/bin/
        cp ${pkgs.pkgsStatic.findutils}/bin/xargs $out/bin/
        cp ${pkgs.pkgsStatic.gnused}/bin/sed $out/bin/
        cp ${pkgs.pkgsStatic.gnugrep}/bin/grep $out/bin/
        cp ${pkgs.pkgsStatic.bash}/bin/bash $out/bin/
        cp ${pkgs.pkgsStatic.gnutar}/bin/tar $out/bin/
        cp ${pkgs.pkgsStatic.pigz}/bin/pigz $out/bin/gzip
        cp ${pkgs.pkgsStatic.bubblewrap}/bin/bwrap $out/bin/
        cp ${pkgs.pkgsStatic.openssh}/bin/* $out/bin/
        cp ${rsyncStatic}/bin/rsync $out/bin/
      '';

  buildLabCoreAndManifest =
    {
      runs,
      includeImages,
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
        inherit pkgs gitHash includeImages;
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
      };
      rootMetadataFilename = builtins.baseNameOf (toString rootMetadata);

      jobs =
        let
          jobPaths = builtins.concatStringsSep " " (map toString allJobDerivations);
        in
        pkgs.runCommand "lab-jobs-all"
          {
            JOB_PATHS = jobPaths;
          }
          ''
            mkdir -p $out
            for job_path in $JOB_PATHS; do
              cp -rL -T "$job_path" "$out/$(basename "$job_path")"
            done
          '';

      labCore = pkgs.stdenv.mkDerivation {
        name = "hpc-lab-core";
        version = labVersion;

        nativeBuildInputs = [
          jobs
          mkHostTools
        ];

        buildCommand = ''
          mkdir -p $out/jobs $out/revision $out/host-tools

          cp -R ${jobs}/* $out/jobs
          cp -r ${mkHostTools} $out/host-tools/$(basename ${mkHostTools})

          cp ${rootMetadata} "$out/revision/${rootMetadataFilename}"

          ${pkgs.lib.concatMapStringsSep "\n" (drv: ''
            cp ${drv} "$out/revision/$(basename ${drv})"
          '') (pkgs.lib.attrValues metadataDrvs)}

          ${pkgs.lib.optionalString includeImages ''
            mkdir -p $out/image
            ${pkgs.lib.concatMapStringsSep "\n" (imageDrv: ''
              image_tarball=$(${pkgs.findutils}/bin/find "${imageDrv}" -name "*.tar.gz" -o -name "*.tar" | head -n 1)
              if [ -z "$image_tarball" ]; then
                echo "Error: Could not find container image tarball in ${imageDrv}"; exit 1;
              fi
              final_filename=$(basename "${imageDrv}")
              image_dir="$out/image/$final_filename"
              mkdir -p "$image_dir"
              ${pkgs.gnutar}/bin/tar -xf "$image_tarball" -C "$image_dir"
            '') imageDerivations}
          ''}
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
    runs:
    let
      artifacts = buildLabCoreAndManifest {
        inherit runs;
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
        version = labVersion;
        nativeBuildInputs = with artifacts; [
          labCore
        ];

        passthru = {
          inherit (artifacts) allJobDerivations;
        };

        buildCommand = ''
          mkdir -p $out $out/lab $out/readme
          cp -rL ${artifacts.labCore}/* $out/

          cp ${artifacts.labManifest} $out/lab/$(basename ${artifacts.labManifest})

          cp ${readme} $out/readme/$(basename ${readme})

          echo "Lab directory created successfully."
        '';
      };
    };
in
{
  inherit runs2Lab;
}
