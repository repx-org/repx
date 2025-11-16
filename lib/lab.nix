{
  pkgs,
  gitHash,
  runs,
}:
let
  lib-lab-internal = (import ./lib-lab-internal.nix) { inherit pkgs gitHash; };
in
lib-lab-internal.runs2Lab runs
// lib-lab-internal.runs2labUnified runs
// lib-lab-internal.runs2LabNative runs
