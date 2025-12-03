{ pkgs, repx-lib }:

{
  deadnix = (import ./checks/deadnix.nix { inherit pkgs; }).lint;
  statix = (import ./checks/statix.nix { inherit pkgs; }).lint;
  formatting = (import ./checks/formatting.nix { inherit pkgs; }).fmt;
  integration = pkgs.callPackage ./checks/check-integration.nix { inherit repx-lib; };
}
// (import ./checks/check-deps.nix { inherit pkgs; })
