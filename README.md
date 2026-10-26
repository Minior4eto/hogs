# hogs

**Find what's hogging your disk.** A fast disk usage TUI for Linux, written in Rust, built to be riced.

- Sorted tree with usage bars, drill in and out with vim keys
- Disk usage *and* apparent size, hard links counted once, symlinks never followed
- Filesystem gauge in the header, live progress while scanning
- Delete safely: `d` moves to the trash, and always asks first
- Themes are plain TOML: colors, borders, bar glyphs, gradients, selector, transparency
- Edit your theme while hogs is open and it reloads live

## Install

```bash
cargo install --path .
# or
cargo build --release && cp target/release/hogs ~/.local/bin/
```

Needs Rust 1.87+.

## Usage

```bash
hogs              # scan the current directory
hogs ~            # scan your home
hogs -x /         # whole root filesystem, without crossing into other mounts
hogs -e .git -e node_modules ~/Projects
hogs --print ~    # top 20 to stdout, no TUI
```

| Key | Action |
|---|---|
| `j` `k` / `↓` `↑` | move |
| `l` `→` `Enter` | open folder |
| `h` `←` `Backspace` | go up |
| `g` / `G` | top / bottom |
| `Ctrl-d` / `Ctrl-u` | half page |
| `s` | sort: size → name → items |
| `a` | disk usage ↔ apparent size |
| `.` | show / hide dotfiles |
| `/` | filter by name |
| `d` | move to trash (asks first) |
| `o` | open with `xdg-open` |
| `t` / `T` | next / previous theme |
| `r` | rescan |
| `?` | help |
| `q` | quit |

## Ricing

Bundled themes: `gruvbox` (default), `catppuccin-mocha`, `tokyo-night`, `nord`, `rose-pine`.
Press `t` inside hogs to cycle through them.

Make your own:

```bash
mkdir -p ~/.config/hogs/themes
hogs --dump-theme gruvbox > ~/.config/hogs/themes/mine.toml
hogs -t mine
```

Keep hogs open in one terminal and your theme in an editor in another: every save reloads instantly.

### Theme reference

Every key is optional; missing ones fall back to Gruvbox.

```toml
[colors]
background   = "#282828"   # "transparent" works too
foreground   = "#ebdbb2"
dim          = "#928374"   # secondary text: percentages, counts, hints
accent       = "#fe8019"   # footer pill, filter, highlights
title        = "#fabd2f"   # panel titles
border       = "#504945"
directory    = "#83a598"
file         = "#ebdbb2"
symlink      = "#8ec07c"
selection_bg = "#3c3836"
selection_fg = "#fbf1c7"
danger       = "#fb4934"   # trash dialog, errors
warning      = "#fabd2f"
bar_low      = "#b8bb26"   # bar color for small entries...
bar_high     = "#fb4934"   # ...blending to this for big ones
bar_empty    = "#3c3836"   # the unfilled track

[style]
border      = "rounded"   # rounded | plain | double | thick | none
bar         = "blocks"    # blocks | thin | ascii | dots
gradient    = "size"      # size: one color per bar | position: fades along the bar
bar_width   = 20          # 4–60
transparent = false       # let Hyprland blur / terminal opacity show through
icons       = false       # Nerd Font icons
selector    = "▌ "
```

Colors can be `#rrggbb`, names (`red`, `lightblue`), or 256-color indexes (`"141"`).
Typos are reported in the footer rather than silently ignored.

### Config

`~/.config/hogs/config.toml`:

```toml
theme = "mine"
show_hidden = true          # default true: ~/.cache is usually the culprit
apparent_size = false
one_filesystem = false
exclude = [".git", "node_modules"]
color = "auto"              # auto | truecolor | 256 | none
```

`NO_COLOR` is respected.

### Previewing a theme without a terminal

```bash
cargo run --example preview -- mine ~ > preview.html
```

## How it works

The scanner walks the tree once with an explicit stack (no recursion) and stores every entry in a flat arena (`Vec<Node>` linked by index), then sums sizes bottom-up in a single reverse pass. The scan runs on a background thread while the UI shows live progress.

## License

MIT
