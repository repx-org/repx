let
  validResourceHintKeys = [
    "mem"
    "cpus"
    "time"
    "partition"
    "sbatch_opts"
  ];
in
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

  mkDependencyMeta =
    { dependencyDerivations, resolvedParameters }:
    let
      depders = dependencyDerivations;
      dependencyPaths = map toString depders;
    in
    {
      inherit dependencyPaths;
      dependencyManifestJson = builtins.toJSON (map builtins.unsafeDiscardStringContext dependencyPaths);
      dependencyHash = builtins.hashString "sha256" (builtins.concatStringsSep ":" dependencyPaths);
      parametersJson = builtins.toJSON resolvedParameters;
    };

  inherit validResourceHintKeys;

  validateResourceHints =
    {
      pkgs,
      resources,
      contextStr,
    }:
    if resources == null || resources == { } then
      resources
    else
      let
        actualKeys = builtins.attrNames resources;
        invalidKeys = pkgs.lib.subtractLists validResourceHintKeys actualKeys;
      in
      if invalidKeys != [ ] then
        throw ''
          Error in ${contextStr}.
          Unknown resource hint keys: ${builtins.toJSON invalidKeys}.
          Valid resource hint keys are: ${builtins.toJSON validResourceHintKeys}.
        ''
      else
        resources;

  mkResolveWithParameters =
    parameters: contextStr: attrName: value:
    if builtins.isFunction value then
      let
        argSet = builtins.functionArgs value;
      in
      if argSet == { parameters = false; } || argSet == { parameters = true; } then
        value { inherit parameters; }
      else
        throw ''
          Stage definition error in '${contextStr}':
          The '${attrName}' attribute is a function, but it must take exactly { parameters } as argument.
          Got function with arguments: ${builtins.toJSON (builtins.attrNames argSet)}
        ''
    else
      value;
}
