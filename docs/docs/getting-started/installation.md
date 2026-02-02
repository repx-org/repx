# Installation

:::info
RepX is currently in **Active Development**. APIs may change between minor versions.
:::

## Prerequisites

To use RepX effectively, you need:

1.  **[Nix](https://nixos.org/download.html)**: The foundation of RepX's reproducibility.
    *   *Requirement:* Flakes must be enabled.
2.  **Rust** (Optional): Only if building the CLI from source.
3.  **Python 3.10+** (Optional): For analyzing results with `repx-py`.

---

## Installing the CLI

Nix is the recommended way to install RepX. It guarantees that your environment matches the experiment definition exactly.

### Method 1: Nix Profile (Recommended)

To install the `repx` CLI globally on your system using Nix profiles:

```bash
nix profile install github:repx-org/repx
```

Verify the installation:
```bash
repx --version
```

### Method 2: NixOS / Home Manager

You can add RepX to your system configuration declaratively.

**flake.nix:**
```nix
{
  inputs.repx.url = "github:repx-org/repx";
  # ...
}
```

**configuration.nix** or **home.nix**:
```nix
environment.systemPackages = [
  inputs.repx.packages.${pkgs.system}.default
];
```

### Method 3: Cargo (Rust)

If you prefer using Rust's package manager, you can install the CLI directly from source.

```bash
# Install from Git
cargo install --git https://github.com/repx-org/repx.git repx-cli
```

---

## Setting up a Project

To use RepX in a project, you need to add it as an input to your `flake.nix`. This allows you to use the **RepX Library** (`repx.lib`) to define your experiments.

**flake.nix:**
```nix
{
  description = "My RepX Experiment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    repx.url = "github:repx-org/repx";
  };

  outputs = { self, nixpkgs, repx }: {
    # 1. Define your Lab (Experiments)
    # See 'User Guide > Defining Experiments' for details.
    
    # 2. (Optional) Create a development shell with the CLI and Python client
    devShells.${nixpkgs.system}.default = nixpkgs.legacyPackages.${nixpkgs.system}.mkShell {
      buildInputs = [
        repx.packages.${nixpkgs.system}.default  # The CLI
        repx.packages.${nixpkgs.system}.repx-py  # The Python Client
      ];
    };
  };
}
```

## Installing the Python Client (`repx-py`)

The Python client allows you to query experiment results and load data into Pandas.

### Inside a Nix Shell (Best Practice)

The best way to use `repx-py` is to include it in your experiment's `devShell` (as shown above). This ensures the library version matches your experiment and CLI.

### Manual Install (pip)

If you are working outside of Nix (e.g., in a global Conda environment or Jupyter Notebook), you can install `repx-py` directly from GitHub:

```bash
pip install "git+https://github.com/repx-org/repx.git#subdirectory=python/src"
```

---

## Upgrading

To upgrade RepX to the latest version:

**Nix Flake:**
```bash
nix flake update repx
```

**Nix Profile:**
```bash
nix profile upgrade repx
```
