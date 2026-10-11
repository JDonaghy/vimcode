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
