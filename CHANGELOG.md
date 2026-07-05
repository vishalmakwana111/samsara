# Changelog

All notable changes to samsara are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.1.1]

### Added
- Branded `--help`: a starfield banner, samsara-palette section colors, and a themed
  examples footer with the star legend.
- "Empty sky" rendering for `list`/`status` when no keys are configured (replaces the plain
  one-line hint).
- Bare `samsara` now shows the full branded help.

## [0.1.0]

Initial release.

### Added
- Key pool management: `add`, `remove`, `list`, `switch`, `status`.
- `daemon` supervisor: watches opencode's `/api/event` SSE stream, detects the Zen usage-limit
  signal (`account_rate_limit` / `free_tier_limit`), and rotates to the next available key.
- Auth swapping: read-modify-write of opencode's `auth.json` `opencode` entry, preserving all
  other providers and keeping mode `0600`.
- Daemon reload via SIGTERM so a running opencode picks up the new key.
- Per-key cooldowns derived from `retry-after` (with a configurable fallback).
- The **constellation** UI: keys as stars (active/ready/cooling), a comet on rebirth,
  truecolor with `NO_COLOR` / `SAMSARA_COLOR` support.
- One-click `install.sh`, CI (fmt/clippy/test on Linux + macOS), and tag-driven release
  binaries for macOS (arm64/x64) and Linux (arm64/x64, musl-static).

[Unreleased]: https://github.com/vishalmakwana111/samsara/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/vishalmakwana111/samsara/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/vishalmakwana111/samsara/releases/tag/v0.1.0
