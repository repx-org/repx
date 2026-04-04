{
  pkgs,
  repx-lib,
  gitHash,
  lab_version,
  runs,
  groups ? { },
  containerMode ? "unified",
  runContainerMode ? "per-run",
}:
let
  validContainerModes = [
    "none"
    "unified"
    "per-run"
  ];

  lab-packagers = (import ./lab-packagers.nix) { inherit pkgs gitHash lab_version; };

  findRunName =
    runPlaceholder:
    runPlaceholder.name or (
      let
        found = pkgs.lib.filterAttrs (_: value: value == runPlaceholder) runs;
      in
      if found == { } then
        throw "Could not find name for run placeholder."
      else
        pkgs.lib.head (pkgs.lib.attrNames found)
    );

  graph = pkgs.lib.mapAttrs (runName: runDef: {
    deps = pkgs.lib.listToAttrs (
      map (
        dep:
        let
          resolved =
            if pkgs.lib.isAttrs dep then
              if (dep._repx_type or "") != "run_placeholder" then
                throw "Invalid dependency in run '${runName}'."
              else
                {
                  obj = dep;
                  type = "hard";
                }
            else if pkgs.lib.isList dep then
              if pkgs.lib.length dep != 2 then
                throw "List dependencies must have exactly 2 elements: [ runObject \"type\" ]."
              else
                let
                  obj = pkgs.lib.elemAt dep 0;
                  type = pkgs.lib.elemAt dep 1;
                in
                if !pkgs.lib.isAttrs obj || (obj._repx_type or "") != "run_placeholder" then
                  throw "First element must be a run object."
                else if !pkgs.lib.isString type then
                  throw "Second element (type) must be a string."
                else
                  { inherit obj type; }
            else
              throw "Dependency must be a run object or [ run type ] list.";

          depRunName = findRunName resolved.obj;
        in
        {
          name = depRunName;
          value = resolved.type;
        }
      ) runDef.dependencies
    );
    placeholder = runDef;
  }) runs;

  isBefore = a: b: builtins.hasAttr a graph."${b}".deps;
  sortResult = pkgs.lib.toposort isBefore (pkgs.lib.attrNames graph);

  sortedRunNames =
    if sortResult ? cycle then
      throw "Circular dependency detected: ${builtins.toJSON sortResult.cycle}"
    else
      sortResult.result;

  runActualNames = pkgs.lib.mapAttrs (
    attrName: runDef:
    let
      runFn =
        if builtins.isFunction runDef.placeholder.runPath then
          runDef.placeholder.runPath
        else
          import runDef.placeholder.runPath;
      runFnArgs = builtins.functionArgs runFn;
      runArgs = pkgs.callPackage runFn (
        {
          inherit pkgs;
        }
        // (
          if runFnArgs ? "repx-lib" || runFnArgs ? "..." then
            {
              repx-lib = repx-lib // {
                utils = repx-lib.mkUtils { inherit pkgs; };
              };
            }
          else
            { }
        )
      );
    in
    runArgs.name or attrName
  ) graph;

  evaluatedRuns = pkgs.lib.listToAttrs (
    map (
      runName:
      let
        runNode = graph."${runName}";
        inherit (runNode) placeholder;

        interRunDepTypes = pkgs.lib.mapAttrs' (
          depKey: depType:
          let
            depActualName = runActualNames.${depKey};
          in
          pkgs.lib.nameValuePair depActualName depType
        ) runNode.deps;

        utils = repx-lib.mkUtils { inherit pkgs; };
        repxLibScope = repx-lib // {
          inherit utils;
        };

        runFn =
          if builtins.isFunction placeholder.runPath then placeholder.runPath else import placeholder.runPath;
        runFnArgs = builtins.functionArgs runFn;
        runArgs = pkgs.callPackage runFn (
          {
            inherit pkgs;
          }
          // (
            if runFnArgs ? "repx-lib" || runFnArgs ? "..." then
              {
                repx-lib = repxLibScope;
              }
            else
              { }
          )
        );

        evaluatedRun = repx-lib.mkRun (
          {
            inherit pkgs;
            repx-lib = repxLibScope;
            name = runName;
            inherit interRunDepTypes;
            dependencyJobs = { };
          }
          // runArgs
        );
      in
      {
        name = runName;
        value = evaluatedRun;
      }
    ) sortedRunNames
  );

  finalRunDefinitions = map (name: evaluatedRuns.${name}) sortedRunNames;

  runNames = map (run: run.name) finalRunDefinitions;
  frequencyMap = pkgs.lib.foldl' (
    acc: name: acc // { "${name}" = (acc."${name}" or 0) + 1; }
  ) { } runNames;
  duplicatesMap = pkgs.lib.filterAttrs (_: count: count > 1) frequencyMap;
  duplicateNames = pkgs.lib.attrNames duplicatesMap;

  resolveGroupPlaceholder =
    groupName: placeholder:
    if !(builtins.isAttrs placeholder) || (placeholder._repx_type or "") != "run_placeholder" then
      throw "Group '${groupName}' contains an element that is not a run placeholder."
    else
      findRunName placeholder;

  resolvedGroups = pkgs.lib.mapAttrs (
    groupName: groupValue:
    if !(pkgs.lib.isList groupValue) then
      throw "Group '${groupName}' must be a list."
    else
      map (resolveGroupPlaceholder groupName) groupValue
  ) groups;

  runNameSet = pkgs.lib.listToAttrs (
    map (name: {
      inherit name;
      value = true;
    }) runNames
  );
  collidingGroupNames = pkgs.lib.filter (gn: runNameSet ? "${gn}") (
    pkgs.lib.attrNames resolvedGroups
  );

in
if !(builtins.elem containerMode validContainerModes) then
  throw ''
    Invalid containerMode "${containerMode}".
    Valid: ${builtins.toJSON validContainerModes}.
  ''
else if !(builtins.elem runContainerMode validContainerModes) then
  throw ''
    Invalid runContainerMode "${runContainerMode}".
    Valid: ${builtins.toJSON validContainerModes}.
  ''
else if duplicateNames != [ ] then
  throw ''
    Duplicate run names: ${builtins.toJSON duplicateNames}
  ''
else if collidingGroupNames != [ ] then
  throw ''
    Group name(s) collide with run name(s): ${builtins.toJSON collidingGroupNames}.
  ''
else
  let
    fullLab = lab-packagers.blueprint2Lab {
      inherit resolvedGroups containerMode;
      runDefinitions = finalRunDefinitions;
    };

    perRunLabs = pkgs.lib.listToAttrs (
      map (runName: {
        name = runName;
        value =
          (lab-packagers.blueprint2Lab {
            containerMode = runContainerMode;
            runDefinitions = [ evaluatedRuns.${runName} ];
          }).lab;
      }) sortedRunNames
    );
  in
  fullLab
  // {
    runs = perRunLabs;
  }
