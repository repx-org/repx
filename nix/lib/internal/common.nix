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

  isVirtualJob = x: builtins.isAttrs x && (x._repx_virtual_job or false);

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

  uniqueJobs =
    jobs:
    builtins.attrValues (
      builtins.listToAttrs (
        map (job: {
          name = job.jobDirName;
          value = job;
        }) jobs
      )
    );

  mkJobId =
    hashInputs:
    let
      rawHash = builtins.hashString "sha256" (builtins.concatStringsSep "\x00" hashInputs);
      nix32 = builtins.convertHash {
        hash = rawHash;
        hashAlgo = "sha256";
        toHashFormat = "nix32";
      };
    in
    builtins.substring 0 32 nix32;

  mkDependencyMeta =
    {
      upstreamJobIds ? [ ],
      dependencyDerivations ? [ ],
      resolvedParameters,
    }:
    let
      dependencyIds =
        if upstreamJobIds != [ ] then
          upstreamJobIds
        else
          map (d: builtins.unsafeDiscardStringContext (toString d)) dependencyDerivations;
    in
    {
      inherit dependencyIds;
      dependencyManifestJson = builtins.toJSON dependencyIds;
      dependencyHash = builtins.hashString "sha256" (builtins.concatStringsSep ":" dependencyIds);
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
