{ pkgs }:
rec {
  range = start: end: pkgs.lib.range start end;

  list = l: {
    _repx_param = true;
    values = l;
    context = [ ];
  };

  scan =
    {
      src,
      type ? "any",
      match ? null,
    }:
    let
      srcStr = toString src;
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
                  targetSrc = src;
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
        if pkgs.lib.isDerivation src then
          [ src ]
        else if isStorePath then
          [ src ]
        else
          [ ];
    in
    {
      _repx_param = true;
      values = result;
      context = contextList;
    };

  dirs =
    src:
    scan {
      inherit src;
      type = "directory";
    };

  files =
    src:
    scan {
      inherit src;
      type = "file";
    };
}
