{ pkgs }:
let
  sanitize =
    p:
    if builtins.isPath p then
      builtins.path {
        path = p;
        name = baseNameOf p;
      }
    else
      p;
in
rec {
  range = start: end: pkgs.lib.range start end;

  list = l: {
    _repx_param = true;
    values = l;
    context = [ ];
  };

  _scanRuntime =
    {
      src,
      type ? "any",
      match ? null,
    }:
    let
      safeSrc = sanitize src;
      srcStr = toString safeSrc;
      isStorePath = pkgs.lib.hasPrefix builtins.storeDir srcStr;

      checkMatch = name: if match == null then true else (builtins.match match name) != null;
      checkType =
        fType:
        if type == "any" then
          true
        else if type == "file" then
          (fType == "regular")
        else if type == "directory" then
          (fType == "directory")
        else
          true;

      result =
        if isStorePath then
          let
            findCmd =
              if type == "directory" then
                "-type d"
              else if type == "file" then
                "-type f"
              else
                "";
            manifest =
              pkgs.runCommand "scan-manifest.json"
                {
                  nativeBuildInputs = [ pkgs.jq ];
                  targetSrc = safeSrc;
                }
                ''
                  cd "$targetSrc"
                  find . -mindepth 1 -maxdepth 1 ${findCmd} | sort | jq -R -s 'split("\n") | map(select(length > 0))' > $out
                '';
            relPaths = builtins.fromJSON (builtins.readFile manifest);
          in
          map (p: "${srcStr}/${p}") (pkgs.lib.filter (p: checkMatch (builtins.baseNameOf p)) relPaths)
        else
          let
            entries = builtins.readDir src;
            filtered = pkgs.lib.filterAttrs (name: fType: (checkType fType) && (checkMatch name)) entries;
            names = builtins.attrNames filtered;
          in
          map (n: "${srcStr}/${n}") names;

      contextList =
        if pkgs.lib.isDerivation safeSrc then
          [ safeSrc ]
        else if isStorePath then
          [ safeSrc ]
        else
          [ ];
    in
    {
      _repx_param = true;
      values = result;
      context = contextList;
    };

  scan = _scanRuntime;
  dirs =
    src:
    let
      isDerivation = pkgs.lib.isDerivation src;
      isStorePathStr = builtins.isString src && pkgs.lib.hasPrefix builtins.storeDir src;
    in
    if isDerivation || isStorePathStr then
      _scanRuntime {
        inherit src;
        type = "directory";
      }
    else
      let
        entries = builtins.readDir src;
        onlyDirs = pkgs.lib.filterAttrs (_: v: v == "directory") entries;
        mkGranularWrapper =
          originalName: _:
          let
            cleanSrc = builtins.path {
              name = "source";
              path = src + "/${originalName}";
            };

            wrapper =
              pkgs.runCommandLocal "submission-wrapper"
                {
                  inherit originalName;
                }
                ''
                  dest="$out/$originalName"
                  mkdir -p "$dest"
                  cp -rT ${cleanSrc} "$dest"
                '';
          in
          {
            path = "${wrapper}/${originalName}";
            drv = wrapper;
          };

        granularObjects = pkgs.lib.mapAttrsToList mkGranularWrapper onlyDirs;
      in
      {
        _repx_param = true;
        values = map (x: x.path) granularObjects;
        context = map (x: x.drv) granularObjects;
      };

  files =
    src:
    _scanRuntime {
      inherit src;
      type = "file";
    };
}
