let
  parseMemToBytes =
    memStr:
    let
      upper = builtins.replaceStrings [ "k" "m" "g" "t" ] [ "K" "M" "G" "T" ] memStr;
      matched = builtins.match "([0-9]+)([KMGT]?)" upper;
    in
    if matched == null then
      null
    else
      let
        numStr = builtins.elemAt matched 0;
        suffix = builtins.elemAt matched 1;
        num = builtins.fromJSON numStr;
        multiplier =
          if suffix == "K" then
            1024
          else if suffix == "M" then
            1024 * 1024
          else if suffix == "G" then
            1024 * 1024 * 1024
          else if suffix == "T" then
            1024 * 1024 * 1024 * 1024
          else
            1;
      in
      num * multiplier;

  parseIntStr =
    str:
    let
      chars = builtins.stringLength str;
      stripped =
        if chars == 0 then
          "0"
        else if chars == 1 then
          str
        else
          let
            findNonZero =
              i:
              if i >= chars then
                chars - 1
              else if builtins.substring i 1 str != "0" then
                i
              else
                findNonZero (i + 1);
            start = findNonZero 0;
          in
          builtins.substring start (chars - start) str;
    in
    builtins.fromJSON stripped;

  parseTimeToSeconds =
    timeStr:
    let
      parts = builtins.filter builtins.isString (builtins.split ":" timeStr);
      numParts = builtins.length parts;
    in
    if numParts == 3 then
      let
        h = parseIntStr (builtins.elemAt parts 0);
        m = parseIntStr (builtins.elemAt parts 1);
        s = parseIntStr (builtins.elemAt parts 2);
      in
      h * 3600 + m * 60 + s
    else if numParts == 2 then
      let
        m = parseIntStr (builtins.elemAt parts 0);
        s = parseIntStr (builtins.elemAt parts 1);
      in
      m * 60 + s
    else if numParts == 1 then
      parseIntStr (builtins.elemAt parts 0)
    else
      0;

  mergeResourceHints =
    a: b:
    let
      aOrEmpty = if a == null then { } else a;
      bOrEmpty = if b == null then { } else b;

      maxMem =
        let
          aBytes = if aOrEmpty ? mem then parseMemToBytes aOrEmpty.mem else null;
          bBytes = if bOrEmpty ? mem then parseMemToBytes bOrEmpty.mem else null;
        in
        if aBytes == null && bBytes == null then
          null
        else if aBytes == null then
          bOrEmpty.mem
        else if bBytes == null then
          aOrEmpty.mem
        else if bBytes > aBytes then
          bOrEmpty.mem
        else
          aOrEmpty.mem;

      maxCpus =
        let
          aCpus = aOrEmpty.cpus or null;
          bCpus = bOrEmpty.cpus or null;
        in
        if aCpus == null && bCpus == null then
          null
        else if aCpus == null then
          bCpus
        else if bCpus == null then
          aCpus
        else if bCpus > aCpus then
          bCpus
        else
          aCpus;

      maxTime =
        let
          aSecs = if aOrEmpty ? time then parseTimeToSeconds aOrEmpty.time else null;
          bSecs = if bOrEmpty ? time then parseTimeToSeconds bOrEmpty.time else null;
        in
        if aSecs == null && bSecs == null then
          null
        else if aSecs == null then
          bOrEmpty.time
        else if bSecs == null then
          aOrEmpty.time
        else if bSecs > aSecs then
          bOrEmpty.time
        else
          aOrEmpty.time;

      finalPartition = bOrEmpty.partition or (aOrEmpty.partition or null);
      finalSbatchOpts = bOrEmpty.sbatch_opts or (aOrEmpty.sbatch_opts or null);
    in
    {
    }
    // (if maxMem != null then { mem = maxMem; } else { })
    // (if maxCpus != null then { cpus = maxCpus; } else { })
    // (if maxTime != null then { time = maxTime; } else { })
    // (if finalPartition != null then { partition = finalPartition; } else { })
    // (if finalSbatchOpts != null then { sbatch_opts = finalSbatchOpts; } else { });

  mergeAllResourceHints = hints: builtins.foldl' (acc: h: mergeResourceHints acc h) { } hints;

  collectInputResources =
    derivations:
    let
      extractHints =
        drv:
        if builtins.isAttrs drv && drv ? passthru && drv.passthru ? resources then
          drv.passthru.resources
        else
          null;
      allHints = builtins.filter (x: x != null) (map extractHints derivations);
    in
    mergeAllResourceHints allHints;

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

  inherit
    parseMemToBytes
    parseTimeToSeconds
    mergeResourceHints
    mergeAllResourceHints
    collectInputResources
    ;

  formatBytesToMem =
    bytes:
    if bytes >= 1024 * 1024 * 1024 * 1024 then
      "${toString (bytes / (1024 * 1024 * 1024 * 1024))}T"
    else if bytes >= 1024 * 1024 * 1024 then
      "${toString (bytes / (1024 * 1024 * 1024))}G"
    else if bytes >= 1024 * 1024 then
      "${toString (bytes / (1024 * 1024))}M"
    else if bytes >= 1024 then
      "${toString (bytes / 1024)}K"
    else
      "${toString bytes}";

  formatSecondsToTime =
    totalSeconds:
    let
      h = totalSeconds / 3600;
      m = (totalSeconds - h * 3600) / 60;
      s = totalSeconds - h * 3600 - m * 60;
      pad = n: if n < 10 then "0${toString n}" else toString n;
    in
    "${pad h}:${pad m}:${pad s}";
}
