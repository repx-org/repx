{ pkgs, repx-lib }:

let
  pkgAlpha = pkgs.runCommand "pkg-alpha" { } ''
    mkdir -p $out/bin
    echo "alpha" > $out/bin/alpha
  '';

  pkgBeta = pkgs.runCommand "pkg-beta" { } ''
    mkdir -p $out/bin
    echo "beta" > $out/bin/beta
  '';

  simpleStage = _: {
    pname = "closure-test-stage";
    version = "1.0";
    parameters = {
      dir = "";
      tag = "";
    };
    outputs = {
      out_dir = "$out";
    };
    run =
      { parameters, outputs, ... }:
      let
        inherit (parameters) dir;
        inherit (parameters) tag;
        inherit (outputs) out_dir;
      in
      ''
        mkdir -p "${out_dir}"
        echo "${tag}" > "${out_dir}/tag.txt"
        if [ -d "${dir}" ]; then
          cp -r "${dir}/." "${out_dir}"
        fi
      '';
  };

  pipeline =
    { repx }:
    {
      step = repx.callStage simpleStage [ ];
    };

  runAlpha = _: {
    name = "closure-test-alpha";
    hashMode = "params-only";
    containerized = true;
    pipelines = [ pipeline ];
    parameters = {
      dir = [ "${pkgAlpha}/bin" ];
      tag = [ "alpha" ];
    };
  };

  runBeta = _: {
    name = "closure-test-beta";
    hashMode = "params-only";
    containerized = true;
    pipelines = [ pipeline ];
    parameters = {
      dir = [ "${pkgBeta}/bin" ];
      tag = [ "beta" ];
    };
  };

  utils = repx-lib.mkUtils { inherit pkgs; };
  repxLibScope = repx-lib // {
    inherit utils;
  };

  evalRun =
    runFn:
    let
      runArgs = pkgs.callPackage runFn { };
    in
    repx-lib.mkRun (
      {
        inherit pkgs;
        repx-lib = repxLibScope;
        dependencyJobs = { };
        interRunDepTypes = { };
      }
      // runArgs
    );

  alphaRun = evalRun runAlpha;
  betaRun = evalRun runBeta;

  isParamDrv =
    d:
    let
      s = builtins.unsafeDiscardStringContext (toString d);
    in
    builtins.match ".*param-store-paths" s != null || builtins.match ".*param-dependencies" s != null;

  paramDrvs = builtins.filter isParamDrv (alphaRun.imageContents ++ betaRun.imageContents);

  mergedParams = pkgs.symlinkJoin {
    name = "merged-param-drvs";
    paths = paramDrvs;
  };

  expectedAlpha = builtins.unsafeDiscardStringContext (toString pkgAlpha);
  expectedBeta = builtins.unsafeDiscardStringContext (toString pkgBeta);

in
pkgs.runCommand "check-image-closure" { } ''
  set -euo pipefail

  echo "=== Image closure regression test ==="
  echo ""
  echo "Checking that symlinkJoin of param derivations preserves all closures..."
  echo ""
  echo "Expected in merged file contents:"
  echo "  ${expectedAlpha}"
  echo "  ${expectedBeta}"
  echo ""

  merged="${mergedParams}"
  echo "Merged param derivations: $merged"
  echo ""

  all_content=$(find -L "$merged" -type f -exec cat {} + 2>/dev/null || true)

  fail=0

  if echo "$all_content" | grep -q "${expectedAlpha}"; then
    echo "  FOUND: pkg-alpha"
  else
    echo "  MISSING: pkg-alpha (not referenced by merged param derivations)"
    fail=1
  fi

  if echo "$all_content" | grep -q "${expectedBeta}"; then
    echo "  FOUND: pkg-beta"
  else
    echo "  MISSING: pkg-beta (not referenced by merged param derivations)"
    fail=1
  fi

  echo ""

  if [ "$fail" -ne 0 ]; then
    echo "FAIL: symlinkJoin collision dropped parameter closure dependencies."
    echo ""
    echo "The param-store-paths derivations from different runs produce"
    echo "identical relative paths, causing lndir to silently skip some."
    echo "Fix: use per-run unique paths in param-store-paths output."
    exit 1
  fi

  echo "PASS: All parameter-referenced packages preserved through symlinkJoin merge."
  mkdir -p $out
  touch $out/passed
''
