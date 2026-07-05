<div align="center">

# ✦ samsara

**Auto-rotating [opencode Zen](https://opencode.ai/docs/zen/) API-key supervisor**

*The endless cycle of death & rebirth — your Zen keys cycle use → limit → cooldown → reborn into rotation.*

[![CI](https://github.com/vishalmakwana111/samsara/actions/workflows/ci.yml/badge.svg)](https://github.com/vishalmakwana111/samsara/actions/workflows/ci.yml)
[![Release](https://github.com/vishalmakwana111/samsara/actions/workflows/release.yml/badge.svg)](https://github.com/vishalmakwana111/samsara/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

</div>

When your active opencode Zen key hits its rolling usage limit (the "12h limit"), opencode
just *waits* — for hours. `samsara` watches for that moment, swaps opencode to your next key,
and keeps you working. Your keys are rendered as a **constellation** of stars: the active one
burns bright, cooling ones dim and brighten again as they near rebirth.

---

## Install

**One click** (macOS & Linux, arm64/x64):

```sh
curl -fsSL https://raw.githubusercontent.com/vishalmakwana111/samsara/master/install.sh | sh
```

Installs the latest release binary to `~/.local/bin`. Override with `SAMSARA_INSTALL_DIR` or
pin a tag with `SAMSARA_VERSION=v0.1.0`.

<details>
<summary>Other ways</summary>

**Prebuilt binaries:** grab a `.tar.gz` from the [latest release](https://github.com/vishalmakwana111/samsara/releases/latest).

**From source** (needs Rust):

```sh
git clone https://github.com/vishalmakwana111/samsara
cd samsara && cargo build --release   # -> target/release/samsara
```

</details>

Make sure `~/.local/bin` is on your `PATH`:

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
```

## Quick start

```sh
samsara add sk-zen-XXXX --label work       # add a key (first becomes active)
samsara add sk-zen-YYYY --label personal
samsara list                               # see the constellation
samsara daemon                             # run the supervisor; auto-rotates on limit
```

Leave `samsara daemon` running in the background (tmux / `&` / a launchd service) alongside
your normal `opencode` usage.

## Commands

| Command | What it does |
|---|---|
| `samsara add <key> [--label NAME]` | Add a Zen key (first one becomes active) |
| `samsara remove <label>` | Remove a key |
| `samsara list` | Render the constellation of keys |
| `samsara status` | Active key, live server, cooldowns + override warnings |
| `samsara switch <label>` | Manually activate a key (swap + reload opencode) |
| `samsara daemon [--default-cooldown 12h] [--dir <path>]` | Run the supervisor |
| `samsara daemon --debug-events` | Print raw opencode SSE events (no rotation) |

## How it works

opencode reads its Zen credential from `~/.local/share/opencode/auth.json` under the
`opencode` provider. samsara keeps its own key pool and:

1. **Watches** opencode's local event stream (`GET /api/event`, SSE, HTTP Basic auth) for the
   usage-limit signal opencode already emits — a session retry with
   `reason: account_rate_limit` / `free_tier_limit`, carrying the reset time.
2. **Rotates** on a hit: cools the exhausted key until reset (from `retry-after`, else a
   configurable default), then rewrites `auth.json` to the next available key — **preserving
   your other providers** (e.g. OpenRouter) and keeping mode `0600`.
3. **Reloads** opencode: a running daemon caches provider config with an *infinite* TTL, so
   samsara restarts it (SIGTERM the registered pid). opencode auto-respawns on the next
   request and re-reads the new key. Sessions are durable, so nothing is lost.

## The look — a constellation

Your keys are stars. Active = bright **gold**, ready = **green**, cooling = dim **cyan** that
brightens as it nears rebirth. On a burnout the daemon streaks a **comet** to the reborn key.

```sh
bash demo-sky.sh    # live preview: twinkling stars + a comet on rebirth (Ctrl-C to stop)
```

Colors are ANSI truecolor and auto-disable when piped or `NO_COLOR` is set; force with
`SAMSARA_COLOR=always|never`.

## Notes & caveats

- **Credential precedence:** if `OPENCODE_API_KEY` is set, or
  `provider.opencode.options.apiKey` is set in `opencode.json`, those OVERRIDE `auth.json` and
  samsara's swaps are ignored. `samsara status` warns about the env var — unset it.
- **Storage:** keys live in `~/.config/samsara/keys.json` (mode `0600`); the active key is
  mirrored into opencode's `auth.json`. Paths honor `XDG_DATA_HOME` / `XDG_STATE_HOME` /
  `XDG_CONFIG_HOME`.
- **Platforms:** macOS & Linux (the daemon reload uses Unix signals).

### Verified vs. pending

Verified against a live opencode v1.17.13 server: server discovery, HTTP Basic auth,
`/api/event` SSE subscription + parsing, `auth.json` read-modify-write (0600, other providers
preserved), key-pool CRUD, cooldown selection, daemon reconnect/backoff.

Pending live confirmation (needs a genuinely exhausted key to trigger a real 429):

1. The exact `account_rate_limit` payload — detection matches it robustly and is unit-tested;
   run `samsara daemon --debug-events` to capture the real event and confirm.
2. **Auto-resume** — after swap+restart, whether the interrupted session continues on its own
   or needs one nudge. A future enhancement can re-drive the session for fully seamless resume.

## Development

```sh
cargo test           # unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

See [CONTRIBUTING.md](CONTRIBUTING.md). Licensed under [MIT](LICENSE).
