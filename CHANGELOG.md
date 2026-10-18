# Changelog

All notable changes to `vimcode` are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This file starts with v0.15.1; earlier releases are not backfilled (see `git
log` for that history).

`## [Unreleased]` is where changelog discipline begins: every PR from this
point forward that changes user-visible behaviour adds a bullet here, in the
same PR. `.github/workflows/release.yml` reads this file's `## [X.Y.Z]`
section to build the GitHub release notes for that version — a release with
no matching section fails the release job before anything is published (see
`scripts/release_notes.py`).

## [Unreleased]

## [0.15.5] - 2026-10-17
### Changed
- "Open Folder" now opens the OS-native folder dialog (NSOpenPanel on
  macOS, GtkFileChooserNative on GTK, IFileOpenDialog on Windows)
  instead of the in-app recursive file picker. TUI falls back to the
  existing in-app picker gracefully.

## [0.15.4] - 2026-10-17
### Added
- Settings panel renders as a centered modal overlay (~80% viewport)
  instead of inside the sidebar. Printable characters filter settings
  immediately (no "/" activation needed), with a "Find: " prefix in the
  search bar. Escape closes; the activity-bar icon toggles.

### Fixed
- Second `yy`/`yw` followed by `p` now correctly pastes the second
  yanked content instead of the first. `sync_register_to_clipboard` only
  tracks the unnamed register, and `load_clipboard_for_paste` no longer
  contaminates the `+`/`*` registers.
- Ex commands and cursor-based file-opening commands (`:e`, `:split`,
  `:vsplit`, `:tabe`/`:tabnew`, `:cd`, `:r`, `:saveas`, `gf`, `gF`,
  `Ctrl-W f`) now expand a leading `~` to the home directory before
  resolving the path, instead of treating it as a literal relative
  directory name (#1847).

## [0.15.3] - 2026-10-14
### Added
- The bottom panel's tab strip now always shows PROBLEMS and OUTPUT tabs
  alongside TERMINAL and DEBUG CONSOLE (when open), instead of a plain
  "Terminal" label with no tab chrome (#1928).
- Declarative skin files: load a skin at startup with `--skin <FILE>` to
  override the chrome colors the GUI backends paint (#1950).
- File picker preview pane now has syntax highlighting (#1926).
- Activity-bar icons are now distinct, non-Codicon glyphs (#1956).
- Release script (`scripts/release.py`) automates the end-to-end release
  workflow including quadraui dependency updates.

### Changed
- The default colorscheme is now `vscode-dark-modern`, matching VS Code's
  current default theme: a neutral dark-grey status bar, title bar and
  activity bar with no saturated blue chrome, with the blue accent reserved
  for the active tab indicator, selection highlight and focus states (#1904).
  Classic Dark+ keeps its existing name, `vscode-dark` (also spellable as
  `dark+` or `vscode-dark-classic`), so anyone who has already set
  `colorscheme` in their config — including to `vscode-dark` — is unaffected;
  only the unset default moves.
- Built-in themes rebranded: `dark` / `light` replace
  `vscode_dark_modern` / `vscode_light` as canonical names (#1951).
- quadraui bumped from 0.1.4 to 0.1.8.

### Fixed
- Status bar, command line, and wildmenu now confined to the main column
  when `status_bar_under_main_column` is enabled — the backdrop no longer
  overspills into the sidebar and activity bar rows (#1959).
- The scroll gutter between the minimap and pane edge now matches the
  editor scrollbar width (16 px on GUI backends) instead of collapsing to
  one character width (~8 px) on macOS (#1960, refs #723, #1869).
- Minimap scroll thumb drag no longer feels sluggish/dampened.
- Minimap thumb drag now uses file-wide geometry — dragging the thumb
  traverses the entire buffer like VS Code, instead of drifting within the
  minimap's painted window.
- `apply_scroll_offset` now accepts both minimap id formats.
- File picker overlay now floats over the editor pane instead of spanning
  the sidebar and full window height (#1925).
- Layout convergence: `sync_per_frame_backend_state` moved to the
  `pre_render` hook so chrome_line_height is set before the shell reads it,
  fixing a one-frame layout lag that caused explorer hit-band drift (#1917).

## [0.15.1] - 2026-10-11

### Changed
- The minimap is now off by default and marked experimental (#1858) —
  enable it with `:set minimap` or `"minimap": true` in `settings.json`.
- VimCode now builds against [quadraui](https://crates.io/crates/quadraui)
  from crates.io (0.1.4, up from 0.1.2).

### Added
- Explorer reveals and highlights the active file when the panel is shown,
  including relative and symlinked paths (#1903).
- The native macOS GUI's editor rows now use VS Code's 1.5× line height
  (18px for Menlo 12), configurable with the new `line_height` setting
  (#1864).
- The bundled icon font is the full Symbols Nerd Font, so extension panel
  icons (e.g. Git Insights) no longer render as `?` in the GUIs (#1853).
- The native Windows and native macOS GUIs are attached as testing-only
  downloads.

### Fixed
- The rust-analyzer "not found" hint now names the workspace root (#1905).
- The TUI in macOS Terminal.app no longer shows runs of dimmed text
  (quadraui#1366).
- The GTK GUI on macOS now shows its icons instead of tofu boxes
  (quadraui#1367).
- The native Windows GUI minimap now shows a real miniature instead of a
  dark strip (quadraui#1354).
