# Release Process

RepX follows Semantic Versioning for all components.

## Version Locations

Update version numbers in the following files:

| File | Field |
|------|-------|
| `Cargo.toml` | `workspace.package.version` |
| `python/src/pyproject.toml` | `project.version` |
| `default.nix` | `version` |
| `nix/lib/lab-packagers.nix` | `labVersion` |

After updating `Cargo.toml`, regenerate the lockfile:

```bash
cargo generate-lockfile
```

## Release Workflow

1. Update version numbers in all locations
2. Commit changes: `git commit -m "chore: bump version to X.Y.Z"`
3. Create annotated tag: `git tag -a vX.Y.Z -m "Release X.Y.Z"`
4. Push with tags: `git push --follow-tags`

## CI/CD Pipeline

GitHub Actions automatically:

- Runs `nix flake check` on all supported platforms
- Builds release binaries via `nix build`
- Deploys documentation to GitHub Pages

## Pre-release Checklist

Before tagging a release:

```bash
# Run full check suite
nix flake check

# Build all packages
nix build .#repx
nix build .#repx-static
nix build .#repx-py
nix build .#docs

# Verify examples
nix run .#check-repx-examples
```
