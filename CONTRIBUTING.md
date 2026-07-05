# Contributing to samsara

Thanks for your interest! samsara is a small, focused Rust CLI.

## Development

```sh
cargo build            # debug build
cargo test             # run unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all        # format (CI enforces --check)
```

CI runs fmt, clippy (warnings denied), and tests on Linux + macOS for every push and PR.

## Layout

| File | Responsibility |
|---|---|
| `src/main.rs` | entry point, logging, CLI dispatch |
| `src/cli.rs` | argument parsing + one-shot command handlers |
| `src/model.rs` | shared types (`KeyEntry`, time/cooldown helpers) |
| `src/keystore.rs` | samsara's key pool + rotation state (`~/.config/samsara/keys.json`) |
| `src/authfile.rs` | read-modify-write of opencode's `auth.json` (preserves other providers) |
| `src/local.rs` | opencode daemon discovery, Basic auth, SIGTERM reload |
| `src/watcher.rs` | the `daemon` loop: SSE subscribe + limit detection |
| `src/rotor.rs` | rotation engine: cooldown → pick next → swap → reload |
| `src/ui.rs` | the constellation (truecolor rendering, comet) |
| `src/paths.rs` | XDG path resolution matching opencode |

## Guidelines

- Keep changes small and focused; add a unit test when you touch logic.
- No `unsafe`. Prefer `anyhow::Result` with `.context(...)` for errors.
- Never log secrets. The UI shows stars, not key values, by design.
- Use [conventional commit](https://www.conventionalcommits.org/) style messages
  (`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`).

## Cutting a release

Releases are tag-driven. Bump the version in `Cargo.toml` and `CHANGELOG.md`, then:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The `Release` workflow builds macOS (arm64/x64) and Linux (arm64/x64, musl-static) binaries
and attaches them (with SHA-256 checksums) to the GitHub Release for that tag.
