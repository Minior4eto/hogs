//! Themes. A theme is one TOML file with a [colors] and a [style] table.
//! Bundled themes are compiled in; files in ~/.config/hogs/themes/ add to or
//! override them. User theme files are watched and reloaded live while hogs runs.

use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, BorderType, Borders};
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;

pub const BUNDLED: &[(&str, &str)] = &[
    ("catppuccin-mocha", include_str!("../themes/catppuccin-mocha.toml")),
    ("gruvbox", include_str!("../themes/gruvbox.toml")),
    ("nord", include_str!("../themes/nord.toml")),
    ("rose-pine", include_str!("../themes/rose-pine.toml")),
    ("tokyo-night", include_str!("../themes/tokyo-night.toml")),
];

pub const DEFAULT_THEME: &str = "gruvbox";

// ── Color capability ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    TrueColor,
    Ansi256,
    None,
}

impl ColorMode {
    pub fn detect(config: Option<&str>) -> Self {
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return Self::None;
        }
        match config {
            Some("truecolor") => return Self::TrueColor,
            Some("256") => return Self::Ansi256,
            Some("none") => return Self::None,
            _ => {}
        }
        let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return Self::TrueColor;
        }
        let term = std::env::var("TERM").unwrap_or_default();
        if ["kitty", "alacritty", "foot", "wezterm", "ghostty"].iter().any(|t| term.contains(t)) {
            return Self::TrueColor;
        }
        Self::Ansi256
    }
}

fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        if r < 8 {
            return 16;
        }
        if r > 248 {
            return 231;
        }
        return 232 + ((r as u16 - 8) * 24 / 247) as u8;
    }
    let q = |v: u8| ((v as u16 * 5 + 127) / 255) as u8;
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

fn convert(color: Color, mode: ColorMode) -> Color {
    match (mode, color) {
        (ColorMode::None, _) => Color::Reset,
        (ColorMode::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(rgb_to_256(r, g, b)),
        _ => color,
    }
}

fn parse_color(value: &str, field: &str) -> Result<Color, String> {
    match value.trim().to_lowercase().as_str() {
        "reset" | "default" | "none" | "transparent" => Ok(Color::Reset),
        _ => Color::from_str(value.trim()).map_err(|_| format!("invalid color for `{field}`: {value:?}")),
    }
}

// ── File format ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    #[serde(default)]
    colors: ColorsFile,
    #[serde(default)]
    style: StyleFile,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ColorsFile {
    background: String,
    foreground: String,
    dim: String,
    accent: String,
    title: String,
    border: String,
    directory: String,
    file: String,
    symlink: String,
    selection_bg: String,
    selection_fg: String,
    danger: String,
    warning: String,
    bar_low: String,
    bar_high: String,
    bar_empty: String,
}

impl Default for ColorsFile {
    // Gruvbox dark, so a theme only has to list what it changes.
    fn default() -> Self {
        let s = |v: &str| v.to_string();
        Self {
            background: s("#282828"),
            foreground: s("#ebdbb2"),
            dim: s("#928374"),
            accent: s("#fe8019"),
            title: s("#fabd2f"),
            border: s("#504945"),
            directory: s("#83a598"),
            file: s("#ebdbb2"),
            symlink: s("#8ec07c"),
            selection_bg: s("#3c3836"),
            selection_fg: s("#fbf1c7"),
            danger: s("#fb4934"),
            warning: s("#fabd2f"),
            bar_low: s("#b8bb26"),
            bar_high: s("#fb4934"),
            bar_empty: s("#3c3836"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StyleFile {
    border: String,
    bar: String,
    gradient: String,
    bar_width: u16,
    transparent: bool,
    icons: bool,
    selector: String,
}

impl Default for StyleFile {
    fn default() -> Self {
        Self {
            border: "rounded".into(),
            bar: "blocks".into(),
            gradient: "size".into(),
            bar_width: 20,
            transparent: false,
            icons: false,
            selector: "▌ ".into(),
        }
    }
}

// ── Resolved theme ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    Blocks,
    Thin,
    Ascii,
    Dots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gradient {
    /// The whole bar takes one color, from bar_low (small) to bar_high (big).
    Size,
    /// Each cell is colored by its position, so long bars fade low → high.
    Position,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub foreground: Color,
    pub dim: Color,
    pub accent: Color,
    pub title: Color,
    pub border: Color,
    pub directory: Color,
    pub file: Color,
    pub symlink: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub danger: Color,
    pub warning: Color,
    pub bar_empty: Color,
    bar_low: Color,
    bar_high: Color,
    pub border_type: Option<BorderType>,
    pub bar: BarStyle,
    pub gradient: Gradient,
    pub bar_width: u16,
    pub icons: bool,
    pub selector: String,
    mode: ColorMode,
}

impl Theme {
    pub fn from_toml(name: &str, text: &str, mode: ColorMode) -> Result<Self, String> {
        Self::parse(name, text, mode).map_err(|e| format!("theme `{name}`: {e}"))
    }

    fn parse(name: &str, text: &str, mode: ColorMode) -> Result<Self, String> {
        let file: ThemeFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
        let c = &file.colors;
        let s = &file.style;
        let col = |value: &str, field: &str| parse_color(value, field).map(|v| convert(v, mode));

        let border_type = match s.border.as_str() {
            "rounded" => Some(BorderType::Rounded),
            "plain" => Some(BorderType::Plain),
            "double" => Some(BorderType::Double),
            "thick" => Some(BorderType::Thick),
            "none" => None,
            other => return Err(format!("unknown border {other:?} (rounded, plain, double, thick, none)")),
        };
        let bar = match s.bar.as_str() {
            "blocks" => BarStyle::Blocks,
            "thin" => BarStyle::Thin,
            "ascii" => BarStyle::Ascii,
            "dots" => BarStyle::Dots,
            other => return Err(format!("unknown bar {other:?} (blocks, thin, ascii, dots)")),
        };
        let gradient = match s.gradient.as_str() {
            "size" => Gradient::Size,
            "position" => Gradient::Position,
            other => return Err(format!("unknown gradient {other:?} (size, position)")),
        };

        Ok(Self {
            name: name.to_string(),
            background: if s.transparent { Color::Reset } else { col(&c.background, "background")? },
            foreground: col(&c.foreground, "foreground")?,
            dim: col(&c.dim, "dim")?,
            accent: col(&c.accent, "accent")?,
            title: col(&c.title, "title")?,
            border: col(&c.border, "border")?,
            directory: col(&c.directory, "directory")?,
            file: col(&c.file, "file")?,
            symlink: col(&c.symlink, "symlink")?,
            selection_bg: col(&c.selection_bg, "selection_bg")?,
            selection_fg: col(&c.selection_fg, "selection_fg")?,
            danger: col(&c.danger, "danger")?,
            warning: col(&c.warning, "warning")?,
            bar_empty: col(&c.bar_empty, "bar_empty")?,
            // Kept unconverted so we can blend between them in RGB.
            bar_low: parse_color(&c.bar_low, "bar_low")?,
            bar_high: parse_color(&c.bar_high, "bar_high")?,
            border_type,
            bar,
            gradient,
            bar_width: s.bar_width.clamp(4, 60),
            icons: s.icons,
            selector: s.selector.clone(),
            mode,
        })
    }

    /// Bar color for a fraction 0.0–1.0, blended between bar_low and bar_high.
    pub fn bar_color(&self, frac: f64) -> Color {
        let frac = if frac.is_finite() { frac.clamp(0.0, 1.0) } else { 0.0 };
        let color = match (self.bar_low, self.bar_high) {
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * frac).round() as u8;
                Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
            }
            (low, high) => {
                if frac < 0.5 {
                    low
                } else {
                    high
                }
            }
        };
        convert(color, self.mode)
    }

    /// Text color to put on top of the accent color (for "pill" labels).
    pub fn on_accent(&self) -> Color {
        match self.background {
            Color::Reset => Color::Black,
            c => c,
        }
    }

    /// A panel styled with this theme's border.
    pub fn block<'a>(&self) -> Block<'a> {
        let block = Block::default().style(Style::default().bg(self.background).fg(self.foreground));
        match self.border_type {
            Some(t) => block.borders(Borders::ALL).border_type(t).border_style(Style::default().fg(self.border)),
            None => block,
        }
    }

    /// Popups always get a border, even when the theme turns borders off.
    pub fn popup_block<'a>(&self, border_color: Color) -> Block<'a> {
        Block::default()
            .style(Style::default().bg(self.background).fg(self.foreground))
            .borders(Borders::ALL)
            .border_type(self.border_type.unwrap_or(BorderType::Plain))
            .border_style(Style::default().fg(border_color))
    }
}

// ── Discovery ─────────────────────────────────────────────────────────────

pub fn user_dir() -> PathBuf {
    crate::config::config_dir().join("themes")
}

pub fn user_path(name: &str) -> Option<PathBuf> {
    let path = user_dir().join(format!("{name}.toml"));
    path.is_file().then_some(path)
}

/// Every theme name: bundled ones plus user files, sorted.
pub fn available() -> Vec<String> {
    let mut names: Vec<String> = BUNDLED.iter().map(|(n, _)| n.to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(user_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// The TOML text of a theme. A user file wins over a bundled theme of the same name.
pub fn source(name: &str) -> Result<String, String> {
    if let Some(path) = user_path(name) {
        return std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()));
    }
    BUNDLED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| text.to_string())
        .ok_or_else(|| format!("unknown theme `{name}` (see `hogs --list-themes`)"))
}

pub fn load(name: &str, mode: ColorMode) -> Result<Theme, String> {
    Theme::from_toml(name, &source(name)?, mode)
}

pub fn fallback(mode: ColorMode) -> Theme {
    let text = BUNDLED.iter().find(|(n, _)| *n == DEFAULT_THEME).unwrap().1;
    Theme::from_toml(DEFAULT_THEME, text, mode).expect("bundled theme is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_parse() {
        for (name, text) in BUNDLED {
            Theme::from_toml(name, text, ColorMode::TrueColor).unwrap();
        }
    }

    #[test]
    fn empty_theme_uses_defaults() {
        let t = Theme::from_toml("x", "", ColorMode::TrueColor).unwrap();
        assert_eq!(t.background, Color::Rgb(0x28, 0x28, 0x28));
    }

    #[test]
    fn typos_are_reported() {
        let err = Theme::from_toml("x", "[colors]\naccnet = \"#fff\"", ColorMode::TrueColor).unwrap_err();
        assert!(err.contains("accnet"), "{err}");
        let err = Theme::from_toml("x", "[colors]\naccent = \"#zzz\"", ColorMode::TrueColor).unwrap_err();
        assert!(err.contains("accent"), "{err}");
    }

    #[test]
    fn gradient_blends() {
        let t = Theme::from_toml(
            "x",
            "[colors]\nbar_low = \"#000000\"\nbar_high = \"#ffffff\"",
            ColorMode::TrueColor,
        )
        .unwrap();
        assert_eq!(t.bar_color(0.0), Color::Rgb(0, 0, 0));
        assert_eq!(t.bar_color(1.0), Color::Rgb(255, 255, 255));
        assert_eq!(t.bar_color(0.5), Color::Rgb(128, 128, 128));
    }

    #[test]
    fn no_color_and_256() {
        let t = Theme::from_toml("x", "", ColorMode::None).unwrap();
        assert_eq!(t.accent, Color::Reset);
        let t = Theme::from_toml("x", "", ColorMode::Ansi256).unwrap();
        assert!(matches!(t.accent, Color::Indexed(_)));
    }
}
