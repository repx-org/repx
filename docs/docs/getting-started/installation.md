# Installation

:::info
RepX is currently in **Active Development**. APIs may change between minor versions.
:::

## Prerequisites

1.  **[Nix](https://nixos.org/download.html)**: The foundation of RepX's reproducibility.
    *   *Requirement:* Flakes must be enabled.
2.  **Python 3.10+** (Optional): For analyzing results with `repx-py`.

---

## Setting up a Project

RepX is used as a **flake input** in your project's `flake.nix`. This is the standard way to use RepX -- it gives you access to the Nix library (`repx.lib`) for defining experiments, the overlay for the CLI and Python client, and ensures everything is version-locked together.

```nix
{
  description = "My RepX Experiment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    repx.url = "github:repx-org/repx";
  };

  outputs = { self, nixpkgs, repx, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ repx.overlays.default ];
      };
      repx-lib = repx.lib;

      labOutputs = (import ./nix/lab.nix) {
        inherit pkgs repx-lib;
        gitHash = self.rev or self.dirtyRev or "unknown";
      };
    in {
      packages.${system} = {
        inherit (labOutputs) lab;
        default = labOutputs.lab;
      };

      devShells.${system}.default = pkgs.mkShell {
        buildInputs = [
          pkgs.repx       # The CLI
          pkgs.repx-py    # The Python client
        ];
      };
    };
}
```

The `devShell` gives you both the `repx` CLI and `repx-py` in your development environment. Enter it with `nix develop` or use [direnv](https://direnv.net/) with an `.envrc` containing `use flake`.

See the [examples/](https://github.com/repx-org/repx/tree/main/examples) directory for complete working projects.

---

## Upgrading

To upgrade RepX to the latest version, update the flake lock:

```bash
nix flake update repx
```

---

## Alternative Installation Methods

:::caution
The methods below install individual components outside of the flake workflow. They are **not officially supported** and may result in version mismatches between the CLI, Nix library, and Python client. Use the flake input approach above for production work.
:::

### Nix Profile (CLI only)

```bash
nix profile install github:repx-org/repx
```

This installs the CLI binary but does **not** provide `repx.lib` or the overlay -- you still need the flake input to define experiments.

### Cargo (CLI only, from source)

```bash
cargo install --git https://github.com/repx-org/repx.git repx-cli
```

### pip (Python client only)

```bash
pip install "git+https://github.com/repx-org/repx.git#subdirectory=python/src"
```
