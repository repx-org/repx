{ pkgs, repx-lib }:

let
  repxLibScope =
    let
      utils = repx-lib.mkUtils { inherit pkgs; };
    in
    repx-lib // { inherit utils; };

  pkgSim = pkgs.runCommand "pkg-sim" { } ''
    mkdir -p $out/bin
    dd if=/dev/zero of=$out/bin/sim bs=1024 count=64
  '';

  pkgAnalysis = pkgs.runCommand "pkg-analysis" { } ''
    mkdir -p $out/bin
    dd if=/dev/zero of=$out/bin/analysis bs=1024 count=32
  '';

  simpleStage = _: {
    pname = "container-mode-test";
    version = "1.0";
    parameters = {
      tag = "";
    };
    outputs = {
      out_dir = "$out";
    };
    run =
      { parameters, outputs, ... }:
      ''
        mkdir -p "${outputs.out_dir}"
        echo "${parameters.tag}" > "${outputs.out_dir}/tag.txt"
      '';
  };

  pipeline =
    { repx }:
    {
      step = repx.callStage simpleStage [ ];
    };

  runSim = _: {
    name = "sim-run";
    pipelines = [ pipeline ];
    parameters = {
      tag = [ "sim" ];
      nix_tool_bin = [ "${pkgSim}/bin" ];
    };
  };

  runAnalysis = _: {
    name = "analysis-run";
    pipelines = [ pipeline ];
    parameters = {
      tag = [ "analysis" ];
      nix_tool_bin = [ "${pkgAnalysis}/bin" ];
    };
  };

  mkTestLab =
    {
      containerMode,
      runContainerMode ? "per-run",
    }:
    repx-lib.mkLab {
      inherit pkgs containerMode runContainerMode;
      repx-lib = repxLibScope;
      gitHash = "container-mode-test";
      lab_version = "1.0.0";
      runs = {
        sim = repx-lib.callRun runSim [ ];
        analysis = repx-lib.callRun runAnalysis [ ];
      };
    };

  labUnified = mkTestLab { containerMode = "unified"; };
  labPerRun = mkTestLab { containerMode = "per-run"; };
  labNone = mkTestLab { containerMode = "none"; };
  labPerRunSlice = mkTestLab {
    containerMode = "unified";
    runContainerMode = "per-run";
  };

in
pkgs.runCommand "check-container-mode"
  {
    nativeBuildInputs = [ pkgs.jq ];
  }
  ''
    set -euo pipefail
    fail=0

    check() {
      local name="$1" cond="$2"
      if eval "$cond"; then
        echo "PASS [$name]"
      else
        echo "FAIL [$name]"
        fail=1
      fi
    }

    echo "=== containerMode tests ==="
    echo ""

    echo "-- unified mode --"
    unified="${labUnified.lab}"

    check "unified: images dir exists" \
      "[ -d '$unified/images' ]"

    unified_image_count=$(find "$unified/images" -maxdepth 1 -mindepth 1 -type d | wc -l)
    check "unified: exactly 1 image" \
      "[ '$unified_image_count' -eq 1 ]"

    sim_image=$(jq -r '.image' "$unified"/revision/*metadata-sim-run*)
    analysis_image=$(jq -r '.image' "$unified"/revision/*metadata-analysis-run*)
    check "unified: both runs share same image" \
      "[ '$sim_image' = '$analysis_image' ]"
    check "unified: image is not null" \
      "[ '$sim_image' != 'null' ]"

    echo ""
    echo "-- per-run mode --"
    perrun="${labPerRun.lab}"

    check "per-run: images dir exists" \
      "[ -d '$perrun/images' ]"

    perrun_image_count=$(find "$perrun/images" -maxdepth 1 -mindepth 1 -type d | wc -l)
    check "per-run: 2 images" \
      "[ '$perrun_image_count' -eq 2 ]"

    sim_image_pr=$(jq -r '.image' "$perrun"/revision/*metadata-sim-run*)
    analysis_image_pr=$(jq -r '.image' "$perrun"/revision/*metadata-analysis-run*)
    check "per-run: images are different" \
      "[ '$sim_image_pr' != '$analysis_image_pr' ]"
    check "per-run: sim image not null" \
      "[ '$sim_image_pr' != 'null' ]"
    check "per-run: analysis image not null" \
      "[ '$analysis_image_pr' != 'null' ]"

    echo ""
    echo "-- none mode --"
    none="${labNone.lab}"

    check "none: no images dir" \
      "[ ! -d '$none/images' ]"

    sim_image_none=$(jq -r '.image' "$none"/revision/*metadata-sim-run*)
    analysis_image_none=$(jq -r '.image' "$none"/revision/*metadata-analysis-run*)
    check "none: sim image is null" \
      "[ '$sim_image_none' = 'null' ]"
    check "none: analysis image is null" \
      "[ '$analysis_image_none' = 'null' ]"

    echo ""
    echo "-- per-run slices --"
    sim_slice="${labPerRunSlice.runs.sim}"
    analysis_slice="${labPerRunSlice.runs.analysis}"

    sim_slice_run_count=$(find "$sim_slice/revision" -name '*metadata-*-run*' | wc -l)
    analysis_slice_run_count=$(find "$analysis_slice/revision" -name '*metadata-*-run*' | wc -l)
    check "slice: sim slice has 1 run metadata" \
      "[ '$sim_slice_run_count' -eq 1 ]"
    check "slice: analysis slice has 1 run metadata" \
      "[ '$analysis_slice_run_count' -eq 1 ]"

    sim_has_sim=$(find "$sim_slice/revision" -name '*metadata-sim-run*' | wc -l)
    sim_has_analysis=$(find "$sim_slice/revision" -name '*metadata-analysis-run*' | wc -l)
    analysis_has_analysis=$(find "$analysis_slice/revision" -name '*metadata-analysis-run*' | wc -l)
    analysis_has_sim=$(find "$analysis_slice/revision" -name '*metadata-sim-run*' | wc -l)
    check "slice: sim slice has sim metadata" \
      "[ '$sim_has_sim' -eq 1 ]"
    check "slice: sim slice has no analysis metadata" \
      "[ '$sim_has_analysis' -eq 0 ]"
    check "slice: analysis slice has analysis metadata" \
      "[ '$analysis_has_analysis' -eq 1 ]"
    check "slice: analysis slice has no sim metadata" \
      "[ '$analysis_has_sim' -eq 0 ]"

    sim_slice_image=$(jq -r '.image' "$sim_slice"/revision/*metadata-sim-run*)
    analysis_slice_image=$(jq -r '.image' "$analysis_slice"/revision/*metadata-analysis-run*)
    check "slice: sim slice has image (per-run)" \
      "[ '$sim_slice_image' != 'null' ]"
    check "slice: analysis slice has image (per-run)" \
      "[ '$analysis_slice_image' != 'null' ]"

    unified_image_dir="$unified/images/$(basename $(ls -d $unified/images/*))"
    sim_slice_image_dir="$sim_slice/images/$(basename $(ls -d $sim_slice/images/*))"

    unified_store_size=$(du -sb "$unified/store" | cut -f1)
    sim_slice_store_size=$(du -sb "$sim_slice/store" | cut -f1)
    check "slice: sim slice store smaller than unified store" \
      "[ '$sim_slice_store_size' -lt '$unified_store_size' ]"

    echo ""
    echo "unified store:    $unified_store_size bytes"
    echo "sim slice store:  $sim_slice_store_size bytes"

    echo ""
    if [ "$fail" -ne 0 ]; then
      echo "SOME TESTS FAILED"
      exit 1
    fi

    echo "All containerMode tests passed."
    mkdir -p $out
    touch $out/passed
  ''
