//! Visual layer: the constellation. Your keys are stars in a night sky —
//! the active key burns bright, ready keys glow steady, cooling keys are dim and
//! slowly brighten as they near rebirth, and a burned-out key gutters to an ember.
//!
//! All output is plain characters + ANSI truecolor. It auto-disables when stdout is
//! not a terminal or `NO_COLOR` is set; force with `SAMSARA_COLOR=always|never`.

use crate::model::{KeyEntry, now_secs};
use std::io::IsTerminal;
use std::sync::OnceLock;

pub type Rgb = (u8, u8, u8);

// palette
pub const GOLD: Rgb = (240, 196, 110);
pub const SAFFRON: Rgb = (245, 158, 66);
pub const VIOLET: Rgb = (150, 120, 232);
pub const ASH: Rgb = (110, 114, 130);
pub const FAINT: Rgb = (72, 76, 92);
pub const GREEN: Rgb = (124, 206, 140);
pub const CYAN: Rgb = (120, 200, 220);
pub const EMBER: Rgb = (232, 120, 92);
pub const WHITE: Rgb = (235, 236, 245);

/// clap help styling in the samsara palette (section headers, usage, flags).
pub fn clap_styles() -> clap::builder::Styles {
    use clap::builder::styling::{Color, RgbColor, Style, Styles};
    let fg = |c: Rgb| Style::new().fg_color(Some(Color::from(RgbColor(c.0, c.1, c.2))));
    Styles::styled()
        .header(fg(GOLD).bold())
        .usage(fg(SAFFRON).bold())
        .literal(fg(GREEN))
        .placeholder(fg(VIOLET))
}

pub fn color_enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| {
        match std::env::var("SAMSARA_COLOR").ok().as_deref() {
            Some("always" | "1" | "yes") => return true,
            Some("never" | "0" | "no") => return false,
            _ => {}
        }
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        std::io::stdout().is_terminal()
    })
}

pub fn paint(c: Rgb, s: &str) -> String {
    if !color_enabled() {
        return s.to_string();
    }
    format!("\x1b[38;2;{};{};{}m{}\x1b[0m", c.0, c.1, c.2, s)
}

pub fn paint_bold(c: Rgb, s: &str) -> String {
    if !color_enabled() {
        return s.to_string();
    }
    format!("\x1b[1;38;2;{};{};{}m{}\x1b[0m", c.0, c.1, c.2, s)
}

pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// Visible width of a string, ignoring ANSI escape sequences.
pub fn visible_len(s: &str) -> usize {
    let mut n = 0usize;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_esc = true;
            continue;
        }
        n += 1;
    }
    n
}

#[derive(Clone, Copy, PartialEq)]
pub enum Star {
    Active,
    Ready,
    Cooling(f32), // progress toward rebirth in [0,1]
}

pub fn star_of(k: &KeyEntry, active: bool, now: u64) -> Star {
    if k.is_cooling(now) {
        Star::Cooling(k.cooldown_progress(now))
    } else if active {
        Star::Active
    } else {
        Star::Ready
    }
}

/// Glyph + colour for a star. `pulse` in [0,1] drives the active-star twinkle.
fn star_render(state: Star, pulse: f32) -> String {
    match state {
        Star::Active => {
            let glyph = if pulse > 0.5 { "✦" } else { "✧" };
            let c = lerp(SAFFRON, GOLD, pulse);
            paint_bold(c, glyph)
        }
        Star::Ready => paint(GREEN, "✦"),
        Star::Cooling(p) => {
            // dim → bright cyan as it nears rebirth
            let c = lerp(FAINT, CYAN, 0.25 + 0.75 * p);
            paint(c, "✦")
        }
    }
}

// Fixed, aesthetically-scattered seats on a 5-row × 26-col sky.
const SEATS: [(usize, usize); 8] = [
    (0, 4),
    (1, 14),
    (0, 21),
    (2, 9),
    (3, 19),
    (1, 1),
    (4, 12),
    (3, 24),
];
// faint background dots (never overlap seats)
const DUST: [(usize, usize); 7] = [(0, 11), (1, 22), (2, 2), (2, 17), (3, 7), (4, 3), (4, 20)];
const SKY_ROWS: usize = 5;
const SKY_COLS: usize = 27;

/// Render the night sky (5 lines) for the given keys at pulse phase `pulse`.
fn render_sky(keys: &[KeyEntry], active: Option<&str>, now: u64, pulse: f32) -> Vec<String> {
    // grid of pre-rendered cells (each a string: colored glyph or space)
    let mut grid: Vec<Vec<String>> = vec![vec![" ".to_string(); SKY_COLS]; SKY_ROWS];
    for (r, c) in DUST {
        grid[r][c] = paint(FAINT, "·");
    }
    for (i, k) in keys.iter().take(SEATS.len()).enumerate() {
        let (r, c) = SEATS[i];
        let is_active = active == Some(k.label.as_str());
        grid[r][c] = star_render(star_of(k, is_active, now), pulse);
    }
    grid.into_iter().map(|row| row.join("")).collect()
}

fn status_chip(k: &KeyEntry, active: bool, now: u64) -> String {
    if k.is_cooling(now) {
        paint(
            CYAN,
            &format!("cooling {}", fmt_dur(k.cooldown_remaining(now))),
        )
    } else if active {
        paint_bold(GOLD, "active")
    } else {
        paint(GREEN, "ready")
    }
}

/// Full constellation view: sky on the left, a legend of stars on the right.
pub fn constellation(keys: &[KeyEntry], active: Option<&str>, pulse: f32) -> String {
    let now = now_secs();
    let sky = render_sky(keys, active, now, pulse);

    // legend rows
    let mut legend: Vec<String> = Vec::new();
    for k in keys.iter().take(SEATS.len()) {
        let is_active = active == Some(k.label.as_str());
        let glyph = star_render(star_of(k, is_active, now), pulse);
        let label = if is_active {
            paint_bold(WHITE, &format!("{:<10}", k.label))
        } else {
            paint(ASH, &format!("{:<10}", k.label))
        };
        legend.push(format!(
            "{glyph} {label} {}",
            status_chip(k, is_active, now)
        ));
    }

    let title = format!(
        "  {} {}",
        paint_bold(GOLD, "✦"),
        paint(ASH, "samsara · the night sky of your keys")
    );

    let rows = sky.len().max(legend.len());
    let mut out = vec![String::new(), title, String::new()];
    for i in 0..rows {
        let left = sky.get(i).cloned().unwrap_or_default();
        let pad = SKY_COLS - visible_len(&left);
        let right = legend.get(i).cloned().unwrap_or_default();
        out.push(format!("  {left}{}   {right}", " ".repeat(pad)));
    }
    out.push(String::new());
    out.join("\n")
}

/// Compact one-line mark for command confirmations.
pub fn mark(glyph_color: Rgb, glyph: &str, msg: &str) -> String {
    format!("  {} {}", paint_bold(glyph_color, glyph), msg)
}

/// A small starfield + wordmark banner, shown atop `--help` and bare `samsara`.
pub fn banner() -> String {
    let word = ["s", "a", "m", "s", "a", "r", "a"]
        .iter()
        .map(|c| paint_bold(GOLD, c))
        .collect::<Vec<_>>()
        .join(" ");
    let l1 = format!(
        "    {}      {}          {}",
        paint(VIOLET, "✦"),
        paint(FAINT, "·"),
        paint(CYAN, "✧")
    );
    let l2 = format!(
        "  {}     {}   {}   {}",
        paint(FAINT, "·"),
        paint_bold(GOLD, "◉"),
        word,
        paint(GREEN, "✦")
    );
    let l3 = format!(
        "       {}      {}      {}",
        paint(CYAN, "✦"),
        paint(FAINT, "·"),
        paint(ASH, "auto-rotating Zen keys · the wheel of keys turns")
    );
    format!("\n{l1}\n{l2}\n{l3}\n")
}

/// Themed example/footer block for `--help`.
pub fn help_footer() -> String {
    let cmd = |s: &str| paint(GREEN, s);
    let note = |s: &str| paint(ASH, s);
    format!(
        "{}\n  {}   {}\n  {}                {}\n  {}                          {}\n\n  {} {}   {} {}   {} {}\n",
        paint_bold(GOLD, "Examples"),
        cmd("samsara add sk-zen-… --label work"),
        note("add your first star"),
        cmd("samsara daemon"),
        note("watch & auto-rotate on limit"),
        cmd("bash demo-sky.sh"),
        note("preview the night sky"),
        paint_bold(GOLD, "✦"),
        note("active"),
        paint(GREEN, "✦"),
        note("ready"),
        paint(CYAN, "✦"),
        note("cooling → reborn on reset"),
    )
}

/// A faint empty night sky with a hint — used when there are no keys yet.
pub fn empty_sky(hint: &[&str]) -> String {
    let dust = [
        format!(
            "   {}      {}        {}",
            paint(FAINT, "·"),
            paint(FAINT, "·"),
            paint(VIOLET, "✧")
        ),
        format!(
            " {}        {}            {}",
            paint(VIOLET, "✦"),
            paint(FAINT, "·"),
            paint(FAINT, "·")
        ),
        format!(
            "      {}         {}    {}",
            paint(FAINT, "·"),
            paint(FAINT, "·"),
            paint(FAINT, "·")
        ),
    ];
    let title = format!(
        "  {} {}",
        paint_bold(GOLD, "✦"),
        paint(ASH, "samsara · an empty sky")
    );
    let mut out = vec![String::new(), title, String::new()];
    for (i, line) in dust.iter().enumerate() {
        let hint_line = hint.get(i).map(|h| paint(ASH, h)).unwrap_or_default();
        out.push(format!("{line}     {hint_line}"));
    }
    out.push(String::new());
    out.join("\n")
}

/// A comet streaking from a burned-out star to the one reborn as active.
/// Single frame (for logs); the live demo animates the head across the tail.
pub fn comet(from: &str, to: &str) -> String {
    let tail = paint(FAINT, "·∙•");
    let head = paint_bold(GOLD, "✦");
    let streak = format!("{tail}{} {head}", paint(SAFFRON, "━━➤"));
    format!(
        "  {}  {}  {}  {}",
        paint(EMBER, &format!("✶ {from}")),
        streak,
        paint_bold(WHITE, to),
        paint(ASH, "reborn")
    )
}

/// Format seconds as a compact human string (e.g. "4h 12m").
pub fn fmt_dur(secs: u64) -> String {
    if secs == 0 {
        return "0s".to_string();
    }
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(format!("{d}d"));
    }
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 && d == 0 {
        parts.push(format!("{m}m"));
    }
    if s > 0 && d == 0 && h == 0 {
        parts.push(format!("{s}s"));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_len_ignores_ansi() {
        let s = paint(GOLD, "✦");
        // one visible glyph regardless of color codes
        assert_eq!(visible_len(&s), 1);
        assert_eq!(visible_len("abc"), 3);
    }

    #[test]
    fn sky_rows_have_consistent_visible_width() {
        let mut keys = Vec::new();
        for l in ["a", "b", "c"] {
            keys.push(KeyEntry {
                label: l.into(),
                key: "k".into(),
                cooling_until: None,
                cooling_since: None,
                last_error: None,
                added_at: None,
            });
        }
        let sky = render_sky(&keys, Some("a"), 0, 1.0);
        assert_eq!(sky.len(), SKY_ROWS);
        for line in &sky {
            assert_eq!(visible_len(line), SKY_COLS);
        }
    }
}
