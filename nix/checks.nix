{ pkgs, repx-lib }:

{
  deadnix = (import ./checks/deadnix.nix { inherit pkgs; }).lint;
  statix = (import ./checks/statix.nix { inherit pkgs; }).lint;
  formatting = (import ./checks/formatting.nix { inherit pkgs; }).fmt;
  shebang = (import ./checks/shebangs.nix { inherit pkgs; }).check;
  shellcheck = (import ./checks/shellcheck.nix { inherit pkgs; }).lint;
  integration = pkgs.callPackage ./checks/check-integration.nix { };
  invalidation = pkgs.callPackage ./checks/check-invalidation.nix { inherit repx-lib; };
  params = pkgs.callPackage ./checks/check-params.nix { };
  pipeline_logic = pkgs.callPackage ./checks/check-pipeline-logic.nix { inherit repx-lib; };
}
// (import ./checks/check-deps.nix { inherit pkgs; })
