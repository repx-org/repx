{
  pkgs,
  repx-lib,
  gitHash,
  lab_version,
  runs,
  groups ? { },
}:
let
  lab-packagers = (import ./lab-packagers.nix) { inherit pkgs gitHash lab_version; };
  findRunName =
    runPlaceholder:
    runPlaceholder.name or (
      let
        found = pkgs.lib.filterAttrs (_: value: value == runPlaceholder) runs;
      in
      if found == { } then
        throw "Could not find name for run placeholder. Ensure all dependencies are part of the `runs` attribute set."
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
                throw "Invalid dependency in run '${runName}'. Expected a run object created by repx-lib.callRun, got an attribute set without _repx_type."
              else
                {
                  obj = dep;
                  type = "hard";
                }

            else if pkgs.lib.isList dep then
              if pkgs.lib.length dep != 2 then
                throw "Invalid dependency in run '${runName}'. List dependencies must have exactly 2 elements: [ runObject \"type\" ]."
              else
                let
                  obj = pkgs.lib.elemAt dep 0;
                  type = pkgs.lib.elemAt dep 1;
                in
                if !pkgs.lib.isAttrs obj || (obj._repx_type or "") != "run_placeholder" then
                  throw "Invalid dependency in run '${runName}'. The first element of the list must be a run object."
                else if !pkgs.lib.isString type then
                  throw "Invalid dependency in run '${runName}'. The second element of the list (type) must be a string."
                else
                  { inherit obj type; }

            else
              throw "Invalid dependency in run '${runName}'. Dependency must be a run object or a [ run type ] list. Got: ${builtins.typeOf dep}";

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
      throw "A circular dependency was detected between runs: ${builtins.toJSON sortResult.cycle}"
    else
      sortResult.result;
  evaluationResults = pkgs.lib.foldl' (
    acc: runName:
    let
      runNode = graph."${runName}";
      inherit (runNode) placeholder;

      interRunDepTypes = pkgs.lib.mapAttrs' (
        depKey: depType:
        let
          depRunDef = acc."${depKey}".evaluatedRun;
          depActualName = depRunDef.name;
        in
        pkgs.lib.nameValuePair depActualName depType
      ) runNode.deps;
      dependencyJobs = pkgs.lib.mapAttrs' (
        depName: _:
        let
          depRunDef = acc."${depName}".evaluatedRun;
          depActualName = depRunDef.name;
        in
        pkgs.lib.nameValuePair depActualName acc."${depName}".jobs
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
          inherit dependencyJobs interRunDepTypes;
        }
        // runArgs
      );
      extractJobs =
        runDef:
        let
          pipelinesForRun = runDef.runs;
          nestedJobs = pkgs.lib.map (pipeline: pkgs.lib.attrValues pipeline) pipelinesForRun;
          allStageResults = pkgs.lib.flatten nestedJobs;
        in
        pkgs.lib.unique (pkgs.lib.filter pkgs.lib.isDerivation allStageResults);
    in
    acc
    // {
      "${runName}" = {
        inherit evaluatedRun;
        jobs = extractJobs evaluatedRun;
      };
    }
  ) { } sortedRunNames;

  finalRunDefinitions = map (name: evaluationResults.${name}.evaluatedRun) sortedRunNames;

  runNames = map (run: run.name) finalRunDefinitions;
  frequencyMap = pkgs.lib.foldl' (
    acc: name: acc // { "${name}" = (acc."${name}" or 0) + 1; }
  ) { } runNames;
  duplicatesMap = pkgs.lib.filterAttrs (_: count: count > 1) frequencyMap;
  duplicateNames = pkgs.lib.attrNames duplicatesMap;

  resolveGroupPlaceholder =
    groupName: placeholder:
    if !(builtins.isAttrs placeholder) || (placeholder._repx_type or "") != "run_placeholder" then
      throw "Error in 'mkLab': group '${groupName}' contains an element that is not a run placeholder created by repx-lib.callRun. Got: ${builtins.typeOf placeholder}"
    else
      let
        runAttrName = findRunName placeholder;
      in
      evaluationResults.${runAttrName}.evaluatedRun.name;

  resolvedGroups = pkgs.lib.mapAttrs (
    groupName: groupValue:
    if !(pkgs.lib.isList groupValue) then
      throw "Error in 'mkLab': group '${groupName}' must be a list of run references, got: ${builtins.typeOf groupValue}"
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
if duplicateNames != [ ] then
  throw ''
    Error: Duplicate run names detected in your lab definition.
    Each run must have a unique 'name' attribute after evaluation.

    The following name(s) were used more than once: ${builtins.toJSON duplicateNames}
  ''
else if collidingGroupNames != [ ] then
  throw ''
    Error in 'mkLab': group name(s) collide with run name(s): ${builtins.toJSON collidingGroupNames}.
    Group names must not match any run name.
  ''
else
  lab-packagers.runs2Lab {
    inherit resolvedGroups;
    runDefinitions = finalRunDefinitions;
  }
