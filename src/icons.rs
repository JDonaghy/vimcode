#![allow(dead_code)]
//! Icon definitions shared by both the GTK and TUI backends.
//!
//! Each `Icon` carries a Nerd Font glyph and a standard Unicode/ASCII fallback.
//! Call `Icon::s()` for `&str` or `Icon::c()` for `char` — these automatically
//! select the right variant based on the global `use_nerd_fonts` flag.
//!
//! Set the flag at startup via `set_nerd_fonts(bool)`.

use std::sync::atomic::{AtomicBool, Ordering};

static USE_NERD_FONTS: AtomicBool = AtomicBool::new(true);

/// Enable or disable Nerd Font glyphs globally.  When disabled, `Icon::s()`
/// and `Icon::c()` return the fallback character instead.
pub fn set_nerd_fonts(val: bool) {
    USE_NERD_FONTS.store(val, Ordering::Relaxed);
}

pub fn nerd_fonts_enabled() -> bool {
    USE_NERD_FONTS.load(Ordering::Relaxed)
}

/// A UI icon with a Nerd Font glyph and a standard-Unicode fallback.
pub struct Icon {
    pub nerd: &'static str,
    pub fallback: &'static str,
}

impl Icon {
    pub const fn new(nerd: &'static str, fallback: &'static str) -> Self {
        Self { nerd, fallback }
    }

    /// Return the icon as a string, selecting nerd or fallback based on the
    /// global flag.
    pub fn s(&self) -> &'static str {
        if USE_NERD_FONTS.load(Ordering::Relaxed) {
            self.nerd
        } else {
            self.fallback
        }
    }

    /// Return the first character of the resolved icon string.
    pub fn c(&self) -> char {
        self.s().chars().next().unwrap_or('?')
    }
}

// ─── Activity Bar ────────────────────────────────────────────────────────────

pub const HAMBURGER: Icon = Icon::new("\u{f035c}", "\u{2630}"); // ☰
pub const EXPLORER: Icon = Icon::new("\u{f07c}", "\u{229e}"); // ⊞
pub const SEARCH: Icon = Icon::new("\u{f002}", "/"); // /
pub const SEARCH_COD: Icon = Icon::new("\u{ea6d}", "/"); // nf-cod-search (GTK only)
pub const DEBUG: Icon = Icon::new("\u{f188}", "!"); // !
pub const GIT_BRANCH: Icon = Icon::new("\u{e702}", "Y"); // Y (branch shape)
pub const GIT_BRANCH_ALT: Icon = Icon::new("\u{e725}", "Y"); // nf-dev-git_branch alt
pub const EXTENSIONS: Icon = Icon::new("\u{eae6}", "#"); // #
pub const EXTENSIONS_ALT: Icon = Icon::new("\u{eb85}", "#"); // nf-cod-extensions alt (TUI)
pub const AI_CHAT: Icon = Icon::new("\u{f0e5}", ">"); // >
pub const SETTINGS: Icon = Icon::new("\u{f013}", "*"); // *

// ─── File Explorer ───────────────────────────────────────────────────────────

pub const FOLDER: Icon = Icon::new("\u{f07b}", "+"); // +
#[allow(dead_code)] // Available for expanded-folder display
pub const FOLDER_OPEN: Icon = Icon::new("\u{f07c}", "-"); // -
pub const FILE_GENERIC: Icon = Icon::new("\u{f15b}", " "); // (space)
pub const FILE_TEXT: Icon = Icon::new("\u{f0f6}", " "); // text file
pub const TRASH: Icon = Icon::new("\u{f1f8}", "x"); // x

// ─── File Type Icons ─────────────────────────────────────────────────────────

pub const FILE_RUST: Icon = Icon::new("\u{e7a8}", "R");
pub const FILE_PYTHON: Icon = Icon::new("\u{f81f}", "P");
pub const FILE_JS: Icon = Icon::new("\u{f81d}", "J");
pub const FILE_TS: Icon = Icon::new("\u{e628}", "T");
pub const FILE_GO: Icon = Icon::new("\u{e724}", "G");
pub const FILE_CPP: Icon = Icon::new("\u{e61d}", "C");
pub const FILE_HEADER: Icon = Icon::new("\u{f0fd}", "H");
pub const FILE_MARKDOWN: Icon = Icon::new("\u{f48a}", "M");
pub const FILE_JSON: Icon = Icon::new("\u{e60b}", "{");
pub const FILE_CONFIG: Icon = Icon::new("\u{e6b2}", "=");
pub const FILE_YAML: Icon = Icon::new("\u{e6a8}", "Y");
pub const FILE_HTML: Icon = Icon::new("\u{f13b}", "<");
pub const FILE_CSS: Icon = Icon::new("\u{e749}", "#");
pub const FILE_SHELL: Icon = Icon::new("\u{f489}", "$");
pub const FILE_LUA: Icon = Icon::new("\u{e620}", "L");

// ─── Debug Toolbar (render.rs DEBUG_BUTTONS) ─────────────────────────────────

pub const DBG_CONTINUE: Icon = Icon::new("\u{f040a}", "\u{25b6}"); // ▶
pub const DBG_PAUSE: Icon = Icon::new("\u{f03e4}", "\u{23f8}"); // ⏸
pub const DBG_STOP: Icon = Icon::new("\u{f04db}", "\u{23f9}"); // ⏹
pub const DBG_RESTART: Icon = Icon::new("\u{f0459}", "\u{21bb}"); // ↻
pub const DBG_STEP_OVER: Icon = Icon::new("\u{f0457}", "\u{2ba9}"); // ⮩
pub const DBG_STEP_OUT: Icon = Icon::new("\u{f0458}", "\u{2ba5}"); // ⮥
pub const DBG_PLAY: Icon = Icon::new("\u{f04b}", "\u{25b6}"); // ▶ (green start)
pub const DBG_STOP_ALT: Icon = Icon::new("\u{f04d}", "\u{25a0}"); // ■ (red stop)

// ─── Debug Sidebar ───────────────────────────────────────────────────────────

pub const DBG_VARIABLES: Icon = Icon::new("\u{f6a9}", "V");
pub const DBG_WATCH: Icon = Icon::new("\u{f06e}", "W");
pub const DBG_CALL_STACK: Icon = Icon::new("\u{f020e}", "S");
pub const DBG_BREAKPOINTS: Icon = Icon::new("\u{f111}", "B");
pub const EXPAND_DOWN: Icon = Icon::new("\u{f0d7} ", "\u{25bc} "); // ▼ (trailing space)
pub const COLLAPSE_RIGHT: Icon = Icon::new("\u{f0da} ", "\u{25b6} "); // ▶ (trailing space)

// ─── Source Control / Git ────────────────────────────────────────────────────

pub const GIT_COMMIT: Icon = Icon::new("\u{e729}", "C");
pub const GIT_PUSH: Icon = Icon::new("\u{f093}", "\u{2191}"); // ↑
pub const GIT_PULL: Icon = Icon::new("\u{f019}", "\u{2193}"); // ↓
pub const GIT_SYNC: Icon = Icon::new("\u{f021}", "~");
pub const GIT_HISTORY: Icon = Icon::new("\u{f417}", "H");
pub const GIT_EDIT: Icon = Icon::new("\u{f044}", "E");
pub const GIT_TAG: Icon = Icon::new("\u{f02b}", "+");
pub const GIT_STAGED: Icon = Icon::new("\u{f055}", "+");

// ─── Editor Features ─────────────────────────────────────────────────────────

pub const LIGHTBULB: Icon = Icon::new("\u{f0eb}", "*");
pub const PLUGIN_FALLBACK: Icon = Icon::new("\u{f03a}", "?");

// ─── Tab Bar / Split Buttons (wide glyphs, TUI) ─────────────────────────────

pub const DIFF_PREV: Icon = Icon::new("\u{F0143}", "<");
pub const DIFF_NEXT: Icon = Icon::new("\u{F0140}", ">");
pub const DIFF_FOLD: Icon = Icon::new("\u{F0233}", "=");
pub const SPLIT_RIGHT: Icon = Icon::new("\u{F0932}", "|");
pub const SPLIT_DOWN: Icon = Icon::new("\u{f0d7}", "_");

// ─── File Icon Lookup ────────────────────────────────────────────────────────

/// Return the icon string for a given file extension.
/// Returns the generic file icon for unknown extensions.
pub fn file_icon(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "rs" => FILE_RUST.s(),
        "py" => FILE_PYTHON.s(),
        "js" | "jsx" | "mjs" | "cjs" => FILE_JS.s(),
        "ts" | "tsx" => FILE_TS.s(),
        "go" => FILE_GO.s(),
        "cpp" | "cc" | "cxx" | "c" => FILE_CPP.s(),
        "h" | "hpp" => FILE_HEADER.s(),
        "md" | "markdown" => FILE_MARKDOWN.s(),
        "json" => FILE_JSON.s(),
        "toml" => FILE_CONFIG.s(),
        "yaml" | "yml" => FILE_YAML.s(),
        "html" | "htm" => FILE_HTML.s(),
        "css" => FILE_CSS.s(),
        "sh" | "bash" | "zsh" => FILE_SHELL.s(),
        "lua" => FILE_LUA.s(),
        "txt" => FILE_TEXT.s(),
        _ => FILE_GENERIC.s(),
    }
}
