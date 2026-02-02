# Impure Incremental Builds

This example demonstrates how to use RepX in an "impure" mode to facilitate rapid development cycles (incremental compilation) by bypassing Nix's strict sandboxing.

**Location:** `examples/impure-incremental`

## The Problem

Standard Nix builds are **hermetic**: source code is copied into the Nix store, and builds happen in a sandbox. This guarantees reproducibility but kills the edit-compile-test cycle for languages like C++ or Rust because `make`/`cargo` cannot reuse artifacts from previous builds. Every change triggers a full rebuild.

## The Solution

We can define a derivation that:
1.  Disables the sandbox (`__noChroot = true`).
2.  Reads the current working directory (`PWD`) to access source files directly.
3.  Runs the build tool (e.g., `make`) in place, allowing it to use existing `.o` files.

## Implementation

### `flake.nix`

We define two packages: `labPure` (for production) and `labImpure` (for development).

```nix
labImpure = (import ./nix/lab.nix) {
  pkgs = pkgsImpure; # Uses the impure overlay
  inherit repx-lib;
};
```

### Impure Overlay (`nix/pkgs/impure-make-pkg.nix`)

The impure package definition accesses the host filesystem.

```nix
{ stdenv }:
let
  # Get the absolute path to the 'src' directory on the host
  localDevPath = builtins.getEnv "PWD" + "/src";
in
stdenv.mkDerivation {
  name = "make-pkg";
  
  # Disable the sandbox
  __noChroot = true;
  
  # Ensure the derivation hash changes on every invocation 
  # (otherwise Nix will cache the result and not rebuild)
  version = builtins.toString builtins.currentTime;

  buildPhase = ''
    echo "Building in ${localDevPath}"
    cd "${localDevPath}"
    
    # Run make directly in the source directory
    make
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp "${localDevPath}/mybinary" $out/bin/make-pkg
  '';
}
```

## Running the Example

To run this example, you must use the `--impure` flag with Nix.

```bash
# Enter the directory
cd examples/impure-incremental

# Run the impure lab app
nix run --impure .
```

Or build manually:

```bash
nix build .#lab-impure --impure
repx run build --lab ./result
```

## Warning

:::warning
**Impure builds are NOT reproducible.**
They depend on the state of your local filesystem, environment variables, and unversioned file changes.
:::

Use this pattern strictly for **local development loops**. When you are ready to publish or run experiments, switch back to the pure pipeline to ensure your results can be verified by others.
