{
  pkgs,
  repxRunner,
  referenceLab,
}:

{
  deadnix = (import ./checks/deadnix.nix { inherit pkgs; }).lint;
  statix = (import ./checks/statix.nix { inherit pkgs; }).lint;
  formatting = (import ./checks/formatting.nix { inherit pkgs; }).fmt;
  shebang = (import ./checks/shebangs.nix { inherit pkgs; }).check;
  shellcheck = (import ./checks/shellcheck.nix { inherit pkgs; }).lint;

  e2e-local = import ./checks/e2e-local.nix {
    inherit pkgs repxRunner referenceLab;
  };

  e2e-remote-local = import ./checks/e2e-remote-local.nix {
    inherit pkgs repxRunner referenceLab;
  };

  e2e-remote-slurm = import ./checks/e2e-remote-slurm.nix {
    inherit pkgs repxRunner referenceLab;
  };

  static-analysis = import ./checks/static-analysis.nix {
    inherit pkgs repxRunner;
  };

  foreign-distro-compat = import ./checks/simulate-non-nixos.nix {
    inherit pkgs repxRunner;
  };
}
