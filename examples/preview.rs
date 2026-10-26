//! Render hogs into an HTML file without a terminal. Handy for checking a theme
//! or making README screenshots.
//!
//!   cargo run --example preview -- <theme> <dir> > preview.html

use hogs::app::App;
use hogs::scan::Options;
use hogs::theme::{self, ColorMode};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use std::path::PathBuf;

fn css(color: Color) -> Option<String> {
    match color {
        Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        _ => None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let theme_name = args.next().unwrap_or_else(|| "gruvbox".into());
    let dir = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let (w, h) = (104, 24);

    let theme = theme::load(&theme_name, ColorMode::TrueColor).expect("theme");
    let page_bg = css(theme.background).unwrap_or_else(|| "#111".into());
    let page_fg = css(theme.foreground).unwrap_or_else(|| "#ddd".into());
    let root = std::fs::canonicalize(&dir).expect("dir");
    let mut app = App::new(root, Options::default(), theme, ColorMode::TrueColor, true, true);
    while app.tree.is_none() {
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.on_tick();
    }
    app.handle_key(KeyEvent::from(KeyCode::Down));

    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| hogs::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();

    let mut html = format!(
        "<!doctype html><meta charset=utf-8><body style=\"margin:0;background:{page_bg}\">\
         <pre style=\"margin:0;padding:12px;font:15px/1.25 'DejaVu Sans Mono',monospace;color:{page_fg}\">"
    );
    for y in 0..h {
        for x in 0..w {
            let cell = &buf[(x, y)];
            let mut style = String::from("display:inline-block;width:1ch;");
            if let Some(c) = css(cell.fg) {
                style += &format!("color:{c};");
            }
            if let Some(c) = css(cell.bg) {
                style += &format!("background:{c};");
            }
            if cell.modifier.contains(Modifier::BOLD) {
                style += "font-weight:bold;";
            }
            let sym = match cell.symbol() {
                "<" => "&lt;",
                ">" => "&gt;",
                "&" => "&amp;",
                s => s,
            };
            html += &format!("<span style=\"{style}\">{sym}</span>");
        }
        html.push('\n');
    }
    html += "</pre>";
    println!("{html}");
}
