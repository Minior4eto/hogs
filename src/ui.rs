//! Drawing. Every color and glyph comes from the theme, so this file is where
//! the rice actually happens.

use crate::app::{App, Mode};
use crate::scan::Kind;
use crate::theme::{BarStyle, Gradient, Theme};
use crate::util::{fmt_size, truncate_left};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, HighlightSpacing, List, ListItem, Paragraph};
use ratatui::Frame;
use std::sync::atomic::Ordering;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let t = app.theme.clone();
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(t.background).fg(t.foreground)), area);

    if app.tree.is_none() {
        draw_scanning(f, app, &t, area);
        return;
    }

    let header_h = if t.border_type.is_some() { 3 } else { 2 };
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(header_h), Constraint::Min(3), Constraint::Length(1)]).areas(area);

    draw_header(f, app, &t, header);
    draw_list(f, app, &t, body);
    draw_footer(f, app, &t, footer);

    match app.mode {
        Mode::Confirm(idx) => draw_confirm(f, app, &t, idx, area),
        Mode::Help => draw_help(f, &t, area),
        _ => {}
    }
}

/// A usage bar `width` cells wide, `frac` (0–1) of it filled.
pub fn bar(t: &Theme, frac: f64, width: usize) -> Vec<Span<'static>> {
    let frac = if frac.is_finite() { frac.clamp(0.0, 1.0) } else { 0.0 };
    let (full_ch, empty_ch) = match t.bar {
        BarStyle::Blocks => ('█', '█'),
        BarStyle::Thin => ('━', '─'),
        BarStyle::Ascii => ('#', '.'),
        BarStyle::Dots => ('⣿', '⣀'),
    };
    let color_at = |cell: usize| match t.gradient {
        Gradient::Size => t.bar_color(frac),
        Gradient::Position => t.bar_color((cell as f64 + 0.5) / width as f64),
    };

    let exact = frac * width as f64;
    let full = (exact.floor() as usize).min(width);
    let mut spans = Vec::new();

    match t.gradient {
        Gradient::Size if full > 0 => {
            spans.push(Span::styled(full_ch.to_string().repeat(full), Style::default().fg(color_at(0))));
        }
        _ => {
            for cell in 0..full {
                spans.push(Span::styled(full_ch.to_string(), Style::default().fg(color_at(cell))));
            }
        }
    }

    let mut used = full;
    // Blocks get sub-cell precision with eighth blocks: ▏▎▍▌▋▊▉
    if t.bar == BarStyle::Blocks && full < width {
        let eighths = ((exact - full as f64) * 8.0) as usize;
        if eighths > 0 {
            const PARTIAL: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
            spans.push(Span::styled(
                PARTIAL[eighths.min(7) - 1].to_string(),
                Style::default().fg(color_at(full)).bg(t.bar_empty),
            ));
            used += 1;
        }
    }
    if used < width {
        spans.push(Span::styled(empty_ch.to_string().repeat(width - used), Style::default().fg(t.bar_empty)));
    }
    spans
}

fn icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Dir => "\u{f07b} ",
        Kind::Symlink => "\u{f0c1} ",
        _ => "\u{f15b} ",
    }
}

fn draw_header(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let tree = app.tree.as_ref().expect("tree is loaded");
    let block = t
        .block()
        .title(Span::styled(" hogs ", Style::default().fg(t.title).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(format!(" {} ", t.name), Style::default().fg(t.dim))).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Right side: filesystem gauge. It gets its space first; the path takes the rest.
    let mut gauge = Vec::new();
    if let Some(fs) = app.fs {
        let frac = fs.used() as f64 / fs.total.max(1) as f64;
        gauge.extend(bar(t, frac, 16));
        gauge.push(Span::styled(
            format!(
                "  {} / {} ({:.0}%) · {} free ",
                fmt_size(fs.used()),
                fmt_size(fs.total),
                frac * 100.0,
                fmt_size(fs.available)
            ),
            Style::default().fg(t.dim),
        ));
    }
    let gauge = Line::from(gauge);
    let gauge_w = (gauge.width() as u16).min(inner.width / 2 + 10).min(inner.width);
    let [left, right] = Layout::horizontal([Constraint::Min(0), Constraint::Length(gauge_w)]).areas(inner);

    let root = truncate_left(&tree.root.display().to_string(), left.width.saturating_sub(3) as usize);
    let path = Line::from(vec![
        Span::raw(" "),
        Span::styled(root, Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(path), left);
    f.render_widget(Paragraph::new(gauge).alignment(Alignment::Right), right);
}

fn draw_list(f: &mut Frame, app: &mut App, t: &Theme, area: Rect) {
    let tree = app.tree.as_ref().expect("tree is loaded");
    let cwd = &tree.nodes[app.cwd];

    let rel = tree.path(app.cwd);
    let rel = rel.strip_prefix(&tree.root).map(|p| p.display().to_string()).unwrap_or_default();
    let title = if rel.is_empty() { " ./ ".to_string() } else { format!(" ./{rel}/ ") };
    let block = t
        .block()
        .title(Span::styled(title, Style::default().fg(t.title).add_modifier(Modifier::BOLD)))
        .title(
            Line::from(Span::styled(format!(" {} ", fmt_size(cwd.size(app.apparent))), Style::default().fg(t.dim)))
                .right_aligned(),
        );
    let inner = block.inner(area);
    app.page = inner.height as usize;

    if app.visible.is_empty() {
        let text = if app.filter.is_empty() { "empty" } else { "no matches" };
        f.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(t.dim))).alignment(Alignment::Center).block(block),
            area,
        );
        return;
    }

    let total = cwd.size(app.apparent).max(1) as f64;
    let selected = app.list.selected();
    let items: Vec<ListItem> = app
        .visible
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            let n = &tree.nodes[i];
            let size = n.size(app.apparent);
            let frac = size as f64 / total;
            let is_sel = selected == Some(row);
            let name_color = if is_sel {
                t.selection_fg
            } else {
                match n.kind {
                    Kind::Dir => t.directory,
                    Kind::Symlink => t.symlink,
                    _ => t.file,
                }
            };

            let mut spans = vec![
                Span::styled(
                    format!("{:>10} ", fmt_size(size)),
                    Style::default().fg(if is_sel { t.selection_fg } else { t.foreground }),
                ),
                Span::styled(format!("{:>5.1}% ", frac * 100.0), Style::default().fg(t.dim)),
            ];
            spans.extend(bar(t, frac, t.bar_width as usize));
            spans.push(Span::raw("  "));
            if t.icons {
                spans.push(Span::styled(icon(n.kind), Style::default().fg(name_color)));
            }
            let mut name = n.name.to_string_lossy().into_owned();
            let mut style = Style::default().fg(name_color);
            if n.is_dir() {
                name.push('/');
                style = style.add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(name, style));
            if n.is_dir() && n.items > 1 {
                spans.push(Span::styled(format!("  {}", n.items - 1), Style::default().fg(t.dim)));
            }
            if n.error {
                spans.push(Span::styled("  ⚠ unreadable", Style::default().fg(t.warning)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(t.selection_bg).add_modifier(Modifier::BOLD))
        .highlight_symbol(t.selector.as_str())
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(list, area, &mut app.list);
}

fn draw_footer(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let tree = app.tree.as_ref().expect("tree is loaded");
    let dim = Style::default().fg(t.dim);
    let sep = || Span::styled("  ·  ", dim);

    let left = if app.mode == Mode::Filter {
        Line::from(vec![
            Span::styled(" / ", Style::default().bg(t.accent).fg(t.on_accent()).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::raw(app.filter.clone()),
            Span::styled("█", Style::default().fg(t.accent)),
            Span::styled("   enter keep · esc clear", dim),
        ])
    } else {
        let cwd = &tree.nodes[app.cwd];
        let mut spans = vec![
            Span::styled(
                format!(" {} ", fmt_size(cwd.size(app.apparent))),
                Style::default().bg(t.accent).fg(t.on_accent()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {} items", app.visible.len()), dim),
            sep(),
            Span::styled(format!("sort {}", app.sort.label()), dim),
            sep(),
            Span::styled(if app.apparent { "apparent" } else { "on disk" }, dim),
        ];
        if !app.show_hidden {
            spans.push(sep());
            spans.push(Span::styled("hidden off", dim));
        }
        if !app.filter.is_empty() {
            spans.push(sep());
            spans.push(Span::styled(format!("filter “{}”", app.filter), Style::default().fg(t.accent)));
        }
        Line::from(spans)
    };

    let right = match &app.message {
        Some(m) => Line::from(Span::styled(
            format!("{} ", m.text),
            Style::default().fg(if m.error { t.danger } else { t.accent }),
        )),
        None => Line::from(vec![
            Span::styled("?", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(" help ", dim),
        ]),
    };
    let right_w = (right.width() as u16 + 1).min(area.width * 2 / 3);
    let [l, r] = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w)]).areas(area);
    f.render_widget(Paragraph::new(left), l);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), r);
}

fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn draw_scanning(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let rect = centered(64, 7, area);
    let items = app.progress.items.load(Ordering::Relaxed);
    let bytes = app.progress.bytes.load(Ordering::Relaxed);
    let current = app.progress.current.lock().map(|p| p.display().to_string()).unwrap_or_default();
    let spin = SPINNER[(app.tick / 2) % SPINNER.len()];

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{spin} "), Style::default().fg(t.accent)),
            Span::styled("scanning ", Style::default().fg(t.foreground)),
            Span::styled(app.root.display().to_string(), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
        ])
        .centered(),
        Line::from(Span::styled(
            format!("{items} items · {}", fmt_size(bytes)),
            Style::default().fg(t.foreground),
        ))
        .centered(),
        Line::from(Span::styled(
            truncate_left(&current, rect.width.saturating_sub(4) as usize),
            Style::default().fg(t.dim),
        ))
        .centered(),
    ];
    let block = t.popup_block(t.border).title(Span::styled(" hogs ", Style::default().fg(t.title)));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

fn draw_confirm(f: &mut Frame, app: &App, t: &Theme, idx: usize, area: Rect) {
    let tree = app.tree.as_ref().expect("tree is loaded");
    let n = &tree.nodes[idx];
    let mut detail = fmt_size(n.size(app.apparent));
    if n.is_dir() {
        detail.push_str(&format!(" · {} items inside", n.items - 1));
    }
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Move "),
            Span::styled(tree.name(idx), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::raw(" to the trash?"),
        ])
        .centered(),
        Line::from(Span::styled(detail, Style::default().fg(t.dim))).centered(),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y ", Style::default().bg(t.danger).fg(t.on_accent()).add_modifier(Modifier::BOLD)),
            Span::raw(" trash it    "),
            Span::styled(" any key ", Style::default().bg(t.selection_bg).fg(t.foreground)),
            Span::raw(" cancel"),
        ])
        .centered(),
    ];
    let rect = centered(56, 8, area);
    f.render_widget(Clear, rect);
    let block = t.popup_block(t.danger).title(Span::styled(" trash ", Style::default().fg(t.danger)));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

const HELP: &[(&str, &str)] = &[
    ("j k  ↓ ↑", "move"),
    ("l  → enter", "open folder"),
    ("h  ← bksp", "go up"),
    ("g  G", "top / bottom"),
    ("ctrl-d ctrl-u", "half page"),
    ("s", "cycle sort: size, name, items"),
    ("a", "disk usage ↔ apparent size"),
    (".", "show / hide dotfiles"),
    ("/", "filter by name"),
    ("d", "move to trash (asks first)"),
    ("o", "open with xdg-open"),
    ("t  T", "next / previous theme"),
    ("r", "rescan"),
    ("q", "quit"),
];

fn draw_help(f: &mut Frame, t: &Theme, area: Rect) {
    let mut lines = vec![Line::from("")];
    for (key, desc) in HELP {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:>14}  "), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
            Span::styled(*desc, Style::default().fg(t.foreground)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("press any key", Style::default().fg(t.dim))).centered());
    let rect = centered(50, lines.len() as u16 + 2, area);
    f.render_widget(Clear, rect);
    let block = t.popup_block(t.accent).title(Span::styled(" keys ", Style::default().fg(t.title)));
    f.render_widget(Paragraph::new(lines).block(block), rect);
}
