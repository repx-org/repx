# Development Setup

## Prerequisites

*   **Nix**: You must have Nix installed with flakes enabled.

## Environment

Enter the development shell to get all necessary tools (Rust, Python, Node.js, etc.):

```bash
nix develop
```

This shell provides:
*   Rust toolchain (cargo, rustc, clippy)
*   Python environment (pytest, pandas)
*   Node.js (for documentation)
*   System dependencies (openssl, pkg-config)

## Building from Source

### Rust Components

```bash
cargo build --workspace
```

### Python Components

```bash
pip install -e ./python/src
```

### Documentation

The documentation is built using Docusaurus.

```bash
cd docs
npm install
npm start
```

Or using the Nix flake app:

```bash
nix run .#docs-preview
```
