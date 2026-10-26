//! Application state and key handling. Nothing in here draws; see ui.rs.

use crate::fsinfo::{self, FsUsage};
use crate::scan::{self, Options, Progress, Tree};
use crate::theme::{self, ColorMode, Theme};
use crate::util::fmt_size;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Size,
    Name,
    Items,
}

impl SortKey {
    fn next(self) -> Self {
        match self {
            Self::Size => Self::Name,
            Self::Name => Self::Items,
            Self::Items => Self::Size,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Size => "size",
            Self::Name => "name",
            Self::Items => "items",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Filter,
    /// Waiting for y/n before trashing this node.
    Confirm(usize),
    Help,
}

pub struct Message {
    pub text: String,
    pub error: bool,
}

pub struct App {
    pub root: PathBuf,
    pub opts: Options,
    pub tree: Option<Tree>,
    pub progress: Arc<Progress>,
    scan_rx: Option<mpsc::Receiver<std::io::Result<Tree>>>,

    /// Node index of the directory being shown.
    pub cwd: usize,
    /// Node indices of the rows on screen, filtered and sorted.
    pub visible: Vec<usize>,
    pub list: ListState,
    /// Height of the list in rows, updated by the UI (for page up/down).
    pub page: usize,

    pub sort: SortKey,
    pub apparent: bool,
    pub show_hidden: bool,
    pub filter: String,
    pub mode: Mode,

    pub theme: Theme,
    pub color_mode: ColorMode,
    theme_mtime: Option<SystemTime>,

    pub message: Option<Message>,
    pub fs: Option<FsUsage>,
    pub tick: usize,
    pub should_quit: bool,
    pub fatal: Option<String>,
}

fn theme_mtime(name: &str) -> Option<SystemTime> {
    theme::user_path(name).and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
}

impl App {
    pub fn new(root: PathBuf, opts: Options, theme: Theme, color_mode: ColorMode, apparent: bool, show_hidden: bool) -> Self {
        let mut app = Self {
            fs: fsinfo::usage(&root),
            root,
            opts,
            tree: None,
            progress: Arc::new(Progress::default()),
            scan_rx: None,
            cwd: 0,
            visible: Vec::new(),
            list: ListState::default(),
            page: 10,
            sort: SortKey::Size,
            apparent,
            show_hidden,
            filter: String::new(),
            mode: Mode::Normal,
            theme_mtime: theme_mtime(&theme.name),
            theme,
            color_mode,
            message: None,
            tick: 0,
            should_quit: false,
            fatal: None,
        };
        app.start_scan();
        app
    }

    // ── Scanning ──────────────────────────────────────────────────────────

    pub fn start_scan(&mut self) {
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let (root, opts, p) = (self.root.clone(), self.opts.clone(), progress.clone());
        std::thread::spawn(move || {
            let _ = tx.send(scan::scan(&root, &opts, &p));
        });
        self.progress = progress;
        self.scan_rx = Some(rx);
        self.tree = None;
        self.cwd = 0;
        self.filter.clear();
        self.mode = Mode::Normal;
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan_rx else { return };
        match rx.try_recv() {
            Ok(Ok(tree)) => {
                if tree.errors > 0 {
                    self.error(format!("{} entries couldn't be read", tree.errors));
                }
                self.tree = Some(tree);
                self.scan_rx = None;
                self.list.select(Some(0));
                self.refresh();
            }
            Ok(Err(e)) => {
                self.fatal = Some(format!("scan failed: {e}"));
                self.should_quit = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.fatal = Some("scanner thread crashed".into());
                self.should_quit = true;
            }
        }
    }

    /// Called every frame: finish scans, hot-reload the theme file.
    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.poll_scan();
        if self.tick.is_multiple_of(5) {
            let mtime = theme_mtime(&self.theme.name);
            if mtime.is_some() && mtime != self.theme_mtime {
                self.theme_mtime = mtime;
                let name = self.theme.name.clone();
                match theme::load(&name, self.color_mode) {
                    Ok(t) => {
                        self.theme = t;
                        self.info(format!("reloaded {name}"));
                    }
                    Err(e) => self.error(e),
                }
            }
        }
    }

    // ── View ──────────────────────────────────────────────────────────────

    /// Recompute which rows are visible and in what order.
    pub fn refresh(&mut self) {
        let Some(tree) = &self.tree else {
            self.visible.clear();
            return;
        };
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<usize> = tree.nodes[self.cwd]
            .children
            .iter()
            .copied()
            .filter(|&i| {
                let n = &tree.nodes[i];
                (self.show_hidden || !n.is_hidden())
                    && (needle.is_empty() || n.name.to_string_lossy().to_lowercase().contains(&needle))
            })
            .collect();

        let ap = self.apparent;
        let lower = |i: usize| tree.nodes[i].name.to_string_lossy().to_lowercase();
        match self.sort {
            SortKey::Size => rows.sort_by(|&a, &b| {
                tree.nodes[b].size(ap).cmp(&tree.nodes[a].size(ap)).then_with(|| lower(a).cmp(&lower(b)))
            }),
            SortKey::Name => rows.sort_by_key(|&i| (!tree.nodes[i].is_dir(), lower(i))),
            SortKey::Items => rows.sort_by(|&a, &b| {
                tree.nodes[b].items.cmp(&tree.nodes[a].items).then_with(|| lower(a).cmp(&lower(b)))
            }),
        }
        self.visible = rows;

        if self.visible.is_empty() {
            self.list.select(None);
        } else {
            let sel = self.list.selected().unwrap_or(0).min(self.visible.len() - 1);
            self.list.select(Some(sel));
        }
    }

    pub fn selected_node(&self) -> Option<usize> {
        self.list.selected().and_then(|i| self.visible.get(i).copied())
    }

    fn move_by(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() as isize - 1;
        let cur = self.list.selected().unwrap_or(0) as isize;
        self.list.select(Some((cur + delta).clamp(0, last) as usize));
    }

    fn enter(&mut self) {
        let Some(idx) = self.selected_node() else { return };
        let Some(tree) = &self.tree else { return };
        if !tree.nodes[idx].is_dir() {
            return;
        }
        self.cwd = idx;
        self.filter.clear();
        self.list.select(Some(0));
        self.refresh();
    }

    fn up(&mut self) {
        let Some(tree) = &self.tree else { return };
        let Some(parent) = tree.nodes[self.cwd].parent else { return };
        let came_from = self.cwd;
        self.cwd = parent;
        self.filter.clear();
        self.refresh();
        // Land on the folder we just left, like ranger/yazi do.
        let pos = self.visible.iter().position(|&i| i == came_from).unwrap_or(0);
        self.list.select(Some(pos));
    }

    // ── Actions ───────────────────────────────────────────────────────────

    pub fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: false });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: true });
    }

    /// Move a node to the trash. Only ever called after the user pressed `y`.
    fn trash(&mut self, idx: usize) {
        let result = {
            let Some(tree) = &mut self.tree else { return };
            if idx == 0 {
                return; // never the scan root itself
            }
            let path = tree.path(idx);
            let name = tree.name(idx);
            let size = fmt_size(tree.nodes[idx].size(self.apparent));
            match trash::delete(&path) {
                Ok(()) => {
                    tree.remove(idx);
                    Ok(format!("trashed {name} ({size}), empty the trash to free the space"))
                }
                Err(e) => Err(format!("couldn't trash {name}: {e}")),
            }
        };
        match result {
            Ok(msg) => {
                self.info(msg);
                self.fs = fsinfo::usage(&self.root);
                self.refresh();
            }
            Err(msg) => self.error(msg),
        }
    }

    fn open(&mut self) {
        let Some(idx) = self.selected_node() else { return };
        let Some(tree) = &self.tree else { return };
        let path = tree.path(idx);
        let spawned = Command::new("xdg-open")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => self.info(format!("opened {}", tree.name(idx))),
            Err(e) => self.error(format!("xdg-open failed: {e}")),
        }
    }

    fn cycle_theme(&mut self, step: isize) {
        let names = theme::available();
        let pos = names.iter().position(|n| *n == self.theme.name).unwrap_or(0) as isize;
        let next = &names[(pos + step).rem_euclid(names.len() as isize) as usize];
        match theme::load(next, self.color_mode) {
            Ok(t) => {
                self.theme_mtime = theme_mtime(next);
                self.theme = t;
                self.info(format!("theme: {next}"));
            }
            Err(e) => self.error(e),
        }
    }

    // ── Keys ──────────────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }
        if self.tree.is_none() {
            if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                self.should_quit = true;
            }
            return;
        }
        match self.mode {
            Mode::Help => self.mode = Mode::Normal,
            Mode::Confirm(idx) => {
                self.mode = Mode::Normal;
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    self.trash(idx);
                } else {
                    self.info("cancelled");
                }
            }
            Mode::Filter => match key.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.mode = Mode::Normal;
                    self.refresh();
                }
                KeyCode::Enter => self.mode = Mode::Normal,
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.refresh();
                }
                KeyCode::Char(c) => {
                    self.filter.push(c);
                    self.list.select(Some(0));
                    self.refresh();
                }
                _ => {}
            },
            Mode::Normal => self.normal_key(key),
        }
    }

    fn normal_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let half = (self.page / 2).max(1) as isize;
        self.message = None;
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.refresh();
                }
            }
            KeyCode::Char('d') if ctrl => self.move_by(half),
            KeyCode::Char('u') if ctrl => self.move_by(-half),
            KeyCode::PageDown => self.move_by(self.page.max(1) as isize),
            KeyCode::PageUp => self.move_by(-(self.page.max(1) as isize)),
            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_by(isize::MIN / 2),
            KeyCode::Char('G') | KeyCode::End => self.move_by(isize::MAX / 2),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.enter(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => self.up(),
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.refresh();
            }
            KeyCode::Char('a') => {
                self.apparent = !self.apparent;
                self.refresh();
            }
            KeyCode::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.refresh();
            }
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(idx) = self.selected_node() {
                    self.mode = Mode::Confirm(idx);
                }
            }
            KeyCode::Char('o') => self.open(),
            KeyCode::Char('t') => self.cycle_theme(1),
            KeyCode::Char('T') => self.cycle_theme(-1),
            KeyCode::Char('r') => self.start_scan(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
    }
}
