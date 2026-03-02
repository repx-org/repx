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

  uniqueDrvs =
    drvs:
    builtins.attrValues (
      builtins.listToAttrs (
        map (drv: {
          name = builtins.unsafeDiscardStringContext (toString drv);
          value = drv;
        }) drvs
      )
    );

  # Create a resolveWithParams function parameterized by the params set and error context string.
  # Usage: mkResolveWithParams resolvedParams "stage name or file" "attrName" value
  mkResolveWithParams =
    params: contextStr: attrName: value:
    if builtins.isFunction value then
      let
        argSet = builtins.functionArgs value;
      in
      if argSet == { params = false; } || argSet == { params = true; } then
        value { inherit params; }
      else
        throw ''
          Stage definition error in '${contextStr}':
          The '${attrName}' attribute is a function, but it must take exactly { params } as argument.
          Got function with arguments: ${builtins.toJSON (builtins.attrNames argSet)}
        ''
    else
      value;
}
