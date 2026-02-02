# Nix Functions Reference

This reference documents the public API exposed by `repx.lib`.

## Experiment Definition

### `repx.mkLab`

Creates the top-level Lab derivation. This is the entry point for your experiment definition in `flake.nix`.

**Arguments:**

*   `pkgs` (Attribute Set): The Nixpkgs package set.
*   `repx-lib` (Attribute Set): The RepX library instance.
*   `runs` (Attribute Set): A dictionary where keys are run names and values are Run objects (created by `import`).

**Returns:**
*   (Derivation): The built Lab derivation containing the experiment graph.

### `repx.mkRun`

Defines a parameterized run.

**Arguments:**

*   `name` (String): The unique name of the run.
*   `pipelines` (List): A list of paths to pipeline definition files.
*   `params` (Attribute Set): A dictionary of parameter lists for parameter sweeping.
    *   Example: `{ seed = [ 1 2 3 ]; model = [ "A" "B" ]; }`
    *   RepX generates the Cartesian product of these lists.

**Returns:**
*   (Attribute Set): A Run definition object.

## Pipeline Construction

### `repx.mkPipe`

Constructs a pipeline from a set of stages. Used inside a pipeline file.

**Arguments:**

*   `stages` (Attribute Set): A recursive attribute set (`rec`) where each key is a stage name and value is a Stage object (returned by `callStage`).

**Returns:**
*   (Attribute Set): A Pipeline definition object.

### `repx.callStage`

Instantiates a stage from a file, resolving dependencies and parameters.

**Arguments:**

*   `path` (Path): The path to the stage definition file (`.nix`).
*   `dependencies` (List): A list of dependencies. Elements can be:
    *   **Stage Object**: Implicit mapping. Matches output names to input names.
    *   **List `[stage source target]`**: Explicit mapping. Maps `source` output of `stage` to `target` input of the current stage.

**Returns:**
*   (Derivation): The instantiated Stage derivation.
