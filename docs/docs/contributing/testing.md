# Testing

## Rust Tests

Run unit and integration tests for the Rust crates:

```bash
cargo test --workspace
```

## Python Tests

Run pytest for `repx-py`:

```bash
cd python
pytest
```

## Nix Checks

The flake defines several checks, including building the example labs:

```bash
nix flake check
```
