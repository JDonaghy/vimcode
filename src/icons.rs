#![allow(dead_code)]
//! Icon definitions shared by both the GTK and TUI backends.
//!
//! Each `Icon` carries a Nerd Font glyph and a standard Unicode/ASCII fallback.
//! Call `Icon::s()` for `&str` or `Icon::c()` for `char` — these automatically
//! select the right variant based on the `use_nerd_fonts` flag.
//!
//! Set the flag at startup via `set_nerd_fonts(bool)`.
//!
//! ## Why thread-local, not process-global (#618)
//!
//! The flag is read on every `Icon::s()`/`Icon::c()` call to choose between
//! the nerd glyph and the ASCII fallback, so it directly determines rendered
//! output (and width, since the two variants differ in width). Both the GTK
//! and TUI backends set it once from `engine.settings.use_nerd_fonts` and
//! then render synchronously on that same thread — there is no cross-thread
//! rendering in this codebase. Storing it thread-local rather than
//! process-global means a test that flips the flag (directly or via
//! `Engine`/`ShellApp` startup) can only ever affect other tests scheduled
//! on that *same* worker thread, never tests running concurrently on other
//! threads in the shared `cargo test` process. That closes off the exact
//! failure shape #615 turned out not to be: a render depending on ambient
//! process-wide state, passing locally and failing non-deterministically in
//! CI depending on core count and scheduling.
use std::cell::Cell;

thread_local! {
    static USE_NERD_FONTS: Cell<bool> = const { Cell::new(true) };
}

/// Enable or disable Nerd Font glyphs on the current thread. When disabled,
/// `Icon::s()` and `Icon::c()` return the fallback character instead.
pub fn set_nerd_fonts(val: bool) {
    USE_NERD_FONTS.with(|f| f.set(val));
}

pub fn nerd_fonts_enabled() -> bool {
    USE_NERD_FONTS.with(|f| f.get())
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
    /// current thread's flag (see module docs).
    pub fn s(&self) -> &'static str {
        if nerd_fonts_enabled() {
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

// ─── Find/Replace ───────────────────────────────────────────────────────────

pub const FIND_REPLACE: Icon = Icon::new("\u{eb3c}", "R1"); // nf-cod-replace
pub const FIND_REPLACE_ALL: Icon = Icon::new("\u{eb3d}", "R*"); // nf-cod-replace_all
pub const FIND_IN_SEL: Icon = Icon::new("\u{eb54}", "\u{2261}"); // ≡ nf-cod-selection
pub const FIND_CLOSE: Icon = Icon::new("\u{ea76}", "\u{00d7}"); // × nf-cod-close

// ─── Window Controls (GTK client-side titlebar, #552) ───────────────────────
// Plain Unicode glyphs — deliberately no nerd-font-only variant since these
// draw at the very top of the window before any font capability probing is
// meaningful, and the shapes read fine as monospace fallback text too.

pub const WINDOW_MINIMIZE: Icon = Icon::new("\u{2500}", "\u{2500}"); // ─
pub const WINDOW_MAXIMIZE: Icon = Icon::new("\u{25a1}", "\u{25a1}"); // □
pub const WINDOW_RESTORE: Icon = Icon::new("\u{29c9}", "\u{29c9}"); // ⧉
pub const WINDOW_CLOSE: Icon = Icon::new("\u{2715}", "\u{00d7}"); // ✕ / ×

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

// ─── File Icon Colours ───────────────────────────────────────────────────────
//
// #703 design note — why these live here, next to `file_icon`, rather than as
// `Theme` fields:
//
// The repo rule is "no hardcoded colours in rendering", and `tab_active_accent`
// is the precedent for a theme-owned tab token. That rule is about *chrome*:
// backgrounds, accents and text that must track the active colour scheme. A
// language badge is not chrome — it is part of the icon's **identity**, the
// same way the glyph is. VS Code's Seti icon theme keeps `.rs` orange and
// `.ts` blue in every one of its built-in themes precisely because users
// recognise files by that colour; making it theme-settable would let a theme
// turn every badge the same shade and destroy the signal the badge exists for.
//
// So the colour is stored beside the glyph it belongs to, in one table, and no
// rendering call site ever names a hex value: `render.rs` asks for
// `icons::file_icon_color(ext)` exactly as it already asks for
// `icons::file_icon(ext)`, and the two can never drift apart. If a future issue
// wants per-theme overrides, the right shape is a `Theme` map that *overrides*
// this table, not a replacement for it.
//
// Palette below is Seti-UI's, which is tuned for dark editor chrome (it is what
// VS Code ships).

/// Seti-UI blue — TypeScript, Python, C/C++, CSS, Lua, Markdown.
pub const ICON_BLUE: (u8, u8, u8) = (0x51, 0x9a, 0xba);
/// Seti-UI green — shell scripts.
pub const ICON_GREEN: (u8, u8, u8) = (0x8d, 0xc1, 0x49);
/// Seti-UI orange — Rust, TOML, HTML.
pub const ICON_ORANGE: (u8, u8, u8) = (0xe3, 0x79, 0x33);
/// Seti-UI purple — C/C++ headers, YAML.
pub const ICON_PURPLE: (u8, u8, u8) = (0xa0, 0x74, 0xc4);
/// Seti-UI yellow — JavaScript, JSON.
pub const ICON_YELLOW: (u8, u8, u8) = (0xcb, 0xcb, 0x41);
/// Seti-UI cyan — Go.
pub const ICON_CYAN: (u8, u8, u8) = (0x51, 0xc9, 0xd4);
/// Seti-UI off-white — plain text and unknown extensions.
pub const ICON_NEUTRAL: (u8, u8, u8) = (0xd4, 0xd7, 0xd6);

/// Return the identity colour (24-bit RGB) for a given file extension's icon.
///
/// Pairs 1:1 with [`file_icon`] — every arm there has an arm here, so a tab's
/// glyph and its colour are always looked up from the same extension string.
/// Unknown extensions get [`ICON_NEUTRAL`], matching [`FILE_GENERIC`].
///
/// Returned as a plain RGB triple rather than `render::Color` so this module
/// stays free of any rendering dependency; `render::tab_icon_color` converts.
pub fn file_icon_color(ext: &str) -> (u8, u8, u8) {
    match ext.to_lowercase().as_str() {
        "rs" => ICON_ORANGE,
        "py" => ICON_BLUE,
        "js" | "jsx" | "mjs" | "cjs" => ICON_YELLOW,
        "ts" | "tsx" => ICON_BLUE,
        "go" => ICON_CYAN,
        "cpp" | "cc" | "cxx" | "c" => ICON_BLUE,
        "h" | "hpp" => ICON_PURPLE,
        "md" | "markdown" => ICON_BLUE,
        "json" => ICON_YELLOW,
        "toml" => ICON_ORANGE,
        "yaml" | "yml" => ICON_PURPLE,
        "html" | "htm" => ICON_ORANGE,
        "css" => ICON_BLUE,
        "sh" | "bash" | "zsh" => ICON_GREEN,
        "lua" => ICON_BLUE,
        "txt" => ICON_NEUTRAL,
        _ => ICON_NEUTRAL,
    }
}

/// Check whether a Nerd Font is installed on Windows by scanning the user and
/// system font directories for font files with "Nerd" in the name.
/// Returns `false` on non-Windows platforms.
#[cfg(target_os = "windows")]
pub fn detect_nerd_font_windows() -> bool {
    use std::fs;
    use std::path::PathBuf;
    // User fonts (Windows 10 1803+, no admin required)
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let user_fonts = PathBuf::from(&local).join("Microsoft\\Windows\\Fonts");
        if let Ok(entries) = fs::read_dir(&user_fonts) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.to_lowercase().contains("nerd") {
                        return true;
                    }
                }
            }
        }
    }
    // System fonts
    let sys_fonts = PathBuf::from("C:\\Windows\\Fonts");
    if let Ok(entries) = fs::read_dir(&sys_fonts) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.to_lowercase().contains("nerd") {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(not(target_os = "windows"))]
pub fn detect_nerd_font_windows() -> bool {
    true // On non-Windows, assume available (GTK bundles, Linux has fontconfig)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: Icon = Icon::new("nerd", "fallback");

    /// New threads default to nerd fonts on, matching the old process-global
    /// default — the thread-local swap (#618) must not change this default.
    #[test]
    fn defaults_to_nerd_fonts_enabled_on_a_fresh_thread() {
        let (enabled, s) = std::thread::spawn(|| (nerd_fonts_enabled(), SAMPLE.s()))
            .join()
            .unwrap();
        assert!(enabled);
        assert_eq!(s, "nerd");
    }

    /// The core #618 guarantee: flipping the flag on one thread must not
    /// leak to a concurrently-running thread. With the old `AtomicBool`
    /// this test would be flaky-by-construction (a race whose outcome
    /// depends on scheduling); with thread-local storage each thread's
    /// view is independent by construction, so it's deterministic.
    #[test]
    fn set_nerd_fonts_does_not_leak_across_threads() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let b1 = barrier.clone();
        let disabling = std::thread::spawn(move || {
            set_nerd_fonts(false);
            b1.wait(); // let the other thread observe state while this thread has it disabled
            b1.wait(); // hold until the other thread has taken its reading
            nerd_fonts_enabled()
        });

        let b2 = barrier.clone();
        let observing = std::thread::spawn(move || {
            b2.wait(); // wait for the other thread to disable on its own thread
            let seen = nerd_fonts_enabled(); // must still be this thread's own default: true
            let icon = SAMPLE.s();
            b2.wait();
            (seen, icon)
        });

        assert!(
            !disabling.join().unwrap(),
            "flag should stay disabled on its own thread"
        );
        let (seen, icon) = observing.join().unwrap();
        assert!(
            seen,
            "a thread that never called set_nerd_fonts must still see the default"
        );
        assert_eq!(icon, "nerd");
    }

    /// Sanity check that `set_nerd_fonts(true)` after a `false` still works
    /// on the same thread (round-trip), independent of thread-local storage
    /// mechanics.
    #[test]
    fn set_nerd_fonts_round_trips_on_the_same_thread() {
        std::thread::spawn(|| {
            set_nerd_fonts(false);
            assert!(!nerd_fonts_enabled());
            assert_eq!(SAMPLE.s(), "fallback");

            set_nerd_fonts(true);
            assert!(nerd_fonts_enabled());
            assert_eq!(SAMPLE.s(), "nerd");
        })
        .join()
        .unwrap();
    }
}
