# Changelog

All notable changes to samsara are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [0.2.1]

### Added
- Per-key **usage intelligence**: the daemon tracks active time, activations, and observed
  activity (events) per key; `samsara stats` shows these plus a **burn rate** (events/active-hour)
  and an average "active time between limits" estimate.
- **Adaptive cooldown**: samsara learns each key's real reset window from observed `retry-after`
  values and uses it as the cooldown fallback instead of the fixed default.

## [0.2.0]

### Added
- `samsara doctor` — live preflight self-check (validates each key against Zen; flags
  `OPENCODE_API_KEY` overrides, duplicate keys, auth.json drift, opencode down).
- `add` now validates the key against the provider and supports `--stdin`, `--provider`, and
  `--no-verify`; duplicate keys are rejected.
- Rotation policy (`round-robin` | `priority`) with `pin`/`unpin`, `disable`/`enable`,
  `priority` commands.
- `samsara stats` and `samsara history` (JSONL rotation log).
- `samsara config` — cooldown, policy, notifications.
- Notifications on rotation/exhaustion: desktop banner (macOS/Linux) + webhook.
- `samsara service install/uninstall/status` — launchd (macOS) / systemd --user (Linux),
  plus a daemon PID file, single-instance guard, and `status` daemon detection.
- `samsara secure enable/disable/status` — optional macOS Keychain storage for secrets.
- `samsara watch` — live full-screen constellation dashboard.
- Multi-provider keys (`opencode` | `openrouter` | `anthropic`) mapping to the right auth.json entry.
- Fallback detection of hard credit/monthly limits; the affected session id is surfaced.

## [0.1.2]

### Added
- `samsara update` — self-update to the latest release: downloads the platform binary,
  verifies its SHA-256, and atomically replaces the running executable (`--force` to reinstall).

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

[Unreleased]: https://github.com/vishalmakwana111/samsara/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/vishalmakwana111/samsara/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/vishalmakwana111/samsara/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/vishalmakwana111/samsara/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/vishalmakwana111/samsara/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/vishalmakwana111/samsara/releases/tag/v0.1.0
