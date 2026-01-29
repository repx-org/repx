{
  mkRuntimePackages = pkgs: [
    pkgs.bash
    pkgs.coreutils
    pkgs.findutils
    pkgs.gnused
    pkgs.gawk
    pkgs.gnugrep
    pkgs.jq
  ];

  validateArgs =
    {
      pkgs,
      name,
      validKeys,
      args,
      contextStr ? "",
    }:
    let
      actualKeys = builtins.attrNames args;
      invalidKeys = pkgs.lib.subtractLists validKeys actualKeys;
    in
    if invalidKeys != [ ] then
      throw ''
        Error in ${name}${if contextStr != "" then " " + contextStr else ""}.
        Unknown attributes were provided: ${builtins.toJSON invalidKeys}.
        The set of valid attributes is: ${builtins.toJSON validKeys}.
      ''
    else
      args;
}
