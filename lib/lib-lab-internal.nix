{
  pkgs,
  gitHash,
}:
let
  mkHostTools =
    pkgs.runCommand "host-tools"
      {
        buildInputs = [
          pkgs.pkgsStatic.coreutils
          pkgs.pkgsStatic.jq
          pkgs.pkgsStatic.findutils
          pkgs.pkgsStatic.gnused
          pkgs.pkgsStatic.gnugrep
          pkgs.pkgsStatic.bash
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
      '';

  collectCommonArtifacts =
    {
      runs,
      includeImages ? true,
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

      jobDerivations = pkgs.lib.unique (
        pkgs.lib.flatten (pkgs.lib.map (run: lib-run-internal.run2Jobs run) runs)
      );

      runsAsAttrSet = pkgs.lib.listToAttrs (
        map (runDef: {
          name = runDef.name;
          value = lib-run-internal.run2Jobs runDef;
        }) runs
      );

      metadata = (import ./metadata.nix) {
        inherit pkgs gitHash includeImages;
      } runs runsAsAttrSet jobDerivations;

      jobs =
        let
          jobPaths = builtins.concatStringsSep " " (map (j: toString j) jobDerivations);
          jobsAll =
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
        in
        jobsAll;

      allReadmeParts = (import ./readme.nix) {
        inherit pkgs jobDerivations;
      };

    in
    {
      inherit metadata jobs allReadmeParts;
    };

  runs2Lab =
    runs:
    let
      commonArtifacts = collectCommonArtifacts {
        inherit runs;
        includeImages = true;
      };
      imageDerivations = pkgs.lib.unique (
        pkgs.lib.filter (i: i != null) (pkgs.lib.map (run: run.image) runs)
      );

      readme = pkgs.runCommand "lab-readme-full" { } ''
        mkdir -p $out
        cat ${commonArtifacts.allReadmeParts.readmeNative}/README.md \
            ${commonArtifacts.allReadmeParts.readmeContainer}/README_container.md > $out/README.md
      '';
    in
    rec {
      inherit (commonArtifacts) metadata jobs;

      lab = pkgs.stdenv.mkDerivation {
        name = "hpc-experiment-lab";
        version = "1.0";

        IMAGE_PATHS = builtins.concatStringsSep " " (map toString imageDerivations);

        buildCommand = ''
          mkdir -p $out/jobs $out/image $out/revision

          for image_path in $IMAGE_PATHS; do
            image_tarball=$(${pkgs.findutils}/bin/find "$image_path" -name "*.tar.gz")
            if [ -z "$image_tarball" ]; then
              echo "Error: Could not find container image tarball in $image_path"; exit 1;
            fi
            final_filename=$(basename "$image_path")
            cp "$image_tarball" "$out/image/$final_filename"
          done
          cp -R ${commonArtifacts.jobs}/* $out/jobs
          metadata_dirname=$(basename ${metadata})
          mkdir -p $out/revision/"$metadata_dirname"/
          cp ${metadata}/metadata.json $out/revision/"$metadata_dirname"/metadata.json

          cp -r ${mkHostTools} $out/host-tools

          echo "Adding README.md..."
          cp ${readme}/README.md $out/README.md

          echo "Lab directory created successfully."
        '';
      };
    };

  runs2labUnified =
    runs:
    let
      commonArtifacts = collectCommonArtifacts {
        inherit runs;
        includeImages = false;
      };
      labContents = pkgs.buildEnv {
        name = "image-contents";
        paths = [
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnused
          pkgs.gawk
        ];
        postBuild = ''
          mkdir -p $out/lab/jobs $out/revision
          cp -R ${commonArtifacts.jobs}/* $out/lab/jobs
          metadata_dirname=$(basename ${commonArtifacts.metadata})
          mkdir -p $out/revision/"$metadata_dirname"/
          cp ${commonArtifacts.metadata}/metadata.json $out/revision/"$metadata_dirname"/metadata.json
        '';
      };
    in
    {
      inherit (commonArtifacts) metadata jobs;

      labUnified = pkgs.dockerTools.buildLayeredImage {
        name = "hpc-experiment-lab-unified";
        tag = "latest";
        contents = [ labContents ];
        config = {
          Cmd = [ "/bin/bash" ];
          WorkingDir = "/lab";
        };
      };
    };

  runs2LabNative =
    runs:
    let
      commonArtifacts = collectCommonArtifacts {
        inherit runs;
        includeImages = false;
      };
    in
    rec {
      inherit (commonArtifacts) metadata jobs;

      labNative = pkgs.stdenv.mkDerivation {
        name = "hpc-experiment-lab-native";
        version = "1.0";

        buildCommand = ''
          mkdir -p $out/jobs $out/revision

          cp -R ${commonArtifacts.jobs}/* $out/jobs

          metadata_dirname=$(basename ${metadata})
          mkdir -p $out/revision/"$metadata_dirname"/
          cp ${metadata}/metadata.json $out/revision/"$metadata_dirname"/metadata.json

          cp -r ${mkHostTools} $out/host-tools

          cp ${commonArtifacts.allReadmeParts.readmeNative}/README.md $out/README.md

          echo "Native lab directory created successfully (without container images)."
        '';
      };
    };
in
{
  inherit runs2Lab runs2labUnified runs2LabNative;
}
