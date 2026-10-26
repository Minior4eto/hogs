use anyhow::{bail, Context, Result};
use clap::Parser;
use hogs::app::App;
use hogs::util::fmt_size;
use hogs::{config, scan, theme, ui};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Find what's hogging your disk.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Directory to scan
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Theme to use (see --list-themes)
    #[arg(short, long)]
    theme: Option<String>,

    /// Show apparent size (file length) instead of disk usage
    #[arg(short, long)]
    apparent: bool,

    /// Stay on one filesystem, like `du -x`
    #[arg(short = 'x', long)]
    one_file_system: bool,

    /// Skip entries with this exact name (repeatable)
    #[arg(short, long, value_name = "NAME")]
    exclude: Vec<String>,

    /// Print the 20 biggest entries and exit, no TUI
    #[arg(short, long)]
    print: bool,

    /// List available themes and exit
    #[arg(long)]
    list_themes: bool,

    /// Print a theme's TOML and exit; a starting point for your own theme
    #[arg(long, value_name = "THEME")]
    dump_theme: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.list_themes {
        for name in theme::available() {
            let tag = if theme::user_path(&name).is_some() { "  (user)" } else { "" };
            println!("{name}{tag}");
        }
        return Ok(());
    }
    if let Some(name) = &cli.dump_theme {
        print!("{}", theme::source(name).map_err(anyhow::Error::msg)?);
        return Ok(());
    }

    let cfg = config::load().map_err(anyhow::Error::msg)?;
    let root = std::fs::canonicalize(&cli.path).with_context(|| format!("can't open {}", cli.path.display()))?;
    if !root.is_dir() {
        bail!("{} is not a directory", root.display());
    }

    let mut exclude = cfg.exclude.clone();
    exclude.extend(cli.exclude);
    let opts = scan::Options { one_filesystem: cli.one_file_system || cfg.one_filesystem, exclude };
    let apparent = cli.apparent || cfg.apparent_size;

    if cli.print {
        return print_top(&root, &opts, apparent);
    }

    let mode = theme::ColorMode::detect(cfg.color.as_deref());
    let theme_name = cli.theme.or(cfg.theme).unwrap_or_else(|| theme::DEFAULT_THEME.into());
    let (theme, warning) = match theme::load(&theme_name, mode) {
        Ok(t) => (t, None),
        Err(e) => (theme::fallback(mode), Some(e)),
    };

    let mut app = App::new(root, opts, theme, mode, apparent, cfg.show_hidden);
    if let Some(w) = warning {
        app.error(w);
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result?;
    if let Some(e) = app.fatal {
        bail!(e);
    }
    Ok(())
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;
        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key);
                }
            }
        }
        app.on_tick();
    }
    Ok(())
}

fn print_top(root: &Path, opts: &scan::Options, apparent: bool) -> Result<()> {
    let tree = scan::scan(root, opts, &scan::Progress::default())?;
    let mut kids = tree.nodes[0].children.clone();
    kids.sort_by_key(|&i| std::cmp::Reverse(tree.nodes[i].size(apparent)));
    println!("{:>10}  {}", fmt_size(tree.nodes[0].size(apparent)), tree.root.display());
    for &i in kids.iter().take(20) {
        let n = &tree.nodes[i];
        let slash = if n.is_dir() { "/" } else { "" };
        println!("{:>10}  {}{slash}", fmt_size(n.size(apparent)), n.name.to_string_lossy());
    }
    if tree.errors > 0 {
        eprintln!("{} entries couldn't be read", tree.errors);
    }
    Ok(())
}
