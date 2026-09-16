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

// #950: GTK's `App::shell_config()` used to carry its own `SEARCH_COD`
// (nf-cod-search, `\u{ea6d}`) instead of this constant, so the activity bar
// showed a different search glyph per backend for no product reason — an
// accidental fork, not a deliberate per-platform choice (nothing about a
// search icon is GTK- or TUI-specific). Converged onto the one table both
// backends already shared for every other activity-bar icon.
pub const SEARCH: Icon = Icon::new("\u{f002}", "/"); // /
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

// #992: expanded extension coverage. Glyphs are nerd-fonts' `seti-*` range
// (the actual Seti-UI icon glyphs, per `glyphnames.json` from the
// ryanoasis/nerd-fonts release) except where noted; colours below are
// Seti-UI's own (see the `ICON_*` doc comments and the design note above
// `file_icon_color`).
pub const FILE_CSHARP: Icon = Icon::new("\u{e648}", "C");
pub const FILE_TERRAFORM: Icon = Icon::new("\u{e69a}", "T");
pub const FILE_JAVA: Icon = Icon::new("\u{e66d}", "J");
pub const FILE_KOTLIN: Icon = Icon::new("\u{e634}", "K");
pub const FILE_SCALA: Icon = Icon::new("\u{e68e}", "S");
pub const FILE_RUBY: Icon = Icon::new("\u{e605}", "R");
pub const FILE_PHP: Icon = Icon::new("\u{e608}", "P");
pub const FILE_SWIFT: Icon = Icon::new("\u{e699}", "S");
pub const FILE_DART: Icon = Icon::new("\u{e64c}", "D");
pub const FILE_SQL: Icon = Icon::new("\u{e64d}", "Q");
pub const FILE_XML: Icon = Icon::new("\u{e619}", "X");
pub const FILE_POWERSHELL: Icon = Icon::new("\u{e683}", "P");
pub const FILE_FSHARP: Icon = Icon::new("\u{e65a}", "F");
pub const FILE_ELIXIR: Icon = Icon::new("\u{e62d}", "E");
pub const FILE_ELIXIR_SCRIPT: Icon = Icon::new("\u{e653}", "E");
// Erlang has no Seti-UI assignment at all (checked `mapping.less` upstream);
// `dev-erlang` is the closest nerd-fonts glyph and `ICON_RED` is reused
// rather than inventing a colour Seti never assigns.
pub const FILE_ERLANG: Icon = Icon::new("\u{e7b1}", "E");
pub const FILE_HASKELL: Icon = Icon::new("\u{e61f}", "H");
pub const FILE_CLOJURE: Icon = Icon::new("\u{e642}", "C");
// Nix has no Seti-UI assignment; `md-nix` (Material Design Icons' NixOS
// glyph) plus a reused `ICON_BLUE` (NixOS's own brand colour is blue) stands
// in rather than inventing a new palette colour for one extension.
pub const FILE_NIX: Icon = Icon::new("\u{f1105}", "N");
// Protobuf has no Seti-UI assignment or dedicated nerd-fonts glyph; `md-protocol`
// is the closest available glyph, `ICON_CYAN` reused (pairs with Go, protobuf's
// most common host language) rather than inventing a colour.
pub const FILE_PROTO: Icon = Icon::new("\u{f0fd8}", "P");
pub const FILE_GRAPHQL: Icon = Icon::new("\u{e662}", "G");
pub const FILE_VUE: Icon = Icon::new("\u{e6a0}", "V");
pub const FILE_SVELTE: Icon = Icon::new("\u{e697}", "S");
pub const FILE_SASS: Icon = Icon::new("\u{e603}", "S");
// Seti-UI's own font maps `.less` to the *same* glyph as `.json`
// (`seti-less` and `seti-json` are both U+E60B in nerd-fonts' `seti-*`
// range) -- not a typo here, that's genuinely how upstream Seti draws it.
pub const FILE_LESS: Icon = Icon::new("\u{e60b}", "L");
pub const FILE_CSV: Icon = Icon::new("\u{e64a}", ",");
pub const FILE_SVG: Icon = Icon::new("\u{e698}", "I");
pub const FILE_IMAGE: Icon = Icon::new("\u{e60d}", "I");
pub const FILE_PDF: Icon = Icon::new("\u{e67d}", "D");
pub const FILE_ARCHIVE: Icon = Icon::new("\u{e6aa}", "Z");

// ─── Filename-Matched File Icons (#992) ─────────────────────────────────────
//
// A handful of well-known files are badged by exact filename in VS Code/
// Seti-UI, not by extension: `Dockerfile`, `Makefile`/`CMakeLists.txt`,
// `LICENSE`, `README`, `.gitignore`/`.gitattributes`. See `filename_icon`/
// `filename_icon_color` below and the module doc's #992 note.
pub const FILE_DOCKER: Icon = Icon::new("\u{e650}", "D");
pub const FILE_MAKEFILE: Icon = Icon::new("\u{e673}", "M");
pub const FILE_LICENSE: Icon = Icon::new("\u{e60a}", "L");
pub const FILE_INFO: Icon = Icon::new("\u{e66a}", "i");
pub const FILE_GIT: Icon = Icon::new("\u{e65d}", "G");

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

// #715: U+2500 BOX DRAWINGS LIGHT HORIZONTAL is a hairline rule, not a
// window-control glyph — at titlebar size it renders as a ~1px line, or
// nothing at all if the resolved UI font has no box-drawing coverage (it
// didn't, which is why minimize alone was invisible while □/✕ painted
// fine). U+2014 EM DASH has the same broad coverage as any other symbol
// glyph in a UI font, and its optical weight actually matches □/✕ beside it.
pub const WINDOW_MINIMIZE: Icon = Icon::new("\u{2014}", "\u{2014}"); // —
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
///
/// This is the extension-only half of the lookup. Most callers want
/// [`file_icon_for_name`] instead, which also matches filename-badged files
/// (`Dockerfile`, `.gitignore`, ...) before falling back to this function.
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
        "cs" => FILE_CSHARP.s(),
        "tf" | "tfvars" => FILE_TERRAFORM.s(),
        "java" => FILE_JAVA.s(),
        "kt" | "kts" => FILE_KOTLIN.s(),
        "scala" => FILE_SCALA.s(),
        "rb" => FILE_RUBY.s(),
        "php" => FILE_PHP.s(),
        "swift" => FILE_SWIFT.s(),
        "dart" => FILE_DART.s(),
        "sql" => FILE_SQL.s(),
        "xml" => FILE_XML.s(),
        "ps1" => FILE_POWERSHELL.s(),
        "fs" | "fsx" => FILE_FSHARP.s(),
        "ex" => FILE_ELIXIR.s(),
        "exs" => FILE_ELIXIR_SCRIPT.s(),
        "erl" => FILE_ERLANG.s(),
        "hs" => FILE_HASKELL.s(),
        "clj" | "cljs" => FILE_CLOJURE.s(),
        "nix" => FILE_NIX.s(),
        "proto" => FILE_PROTO.s(),
        "graphql" | "gql" => FILE_GRAPHQL.s(),
        "vue" => FILE_VUE.s(),
        "svelte" => FILE_SVELTE.s(),
        "scss" | "sass" => FILE_SASS.s(),
        "less" => FILE_LESS.s(),
        "ini" | "cfg" | "conf" => FILE_CONFIG.s(),
        "csv" => FILE_CSV.s(),
        "log" => FILE_TEXT.s(),
        "svg" => FILE_SVG.s(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => FILE_IMAGE.s(),
        "pdf" => FILE_PDF.s(),
        "zip" | "tar" | "gz" => FILE_ARCHIVE.s(),
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
/// Seti-UI red (`@red` in `ui-variables.less`) — Java, Ruby, Scala, Svelte, PDF.
/// #992: not present before because nothing in the pre-#992 table used it.
pub const ICON_RED: (u8, u8, u8) = (0xcc, 0x3e, 0x44);
/// Seti-UI pink (`@pink`) — GraphQL, Sass/SCSS, SQL. #992.
pub const ICON_PINK: (u8, u8, u8) = (0xf5, 0x53, 0x85);
/// Seti-UI grey-light (`@grey-light`) — config/cfg/conf/ini, editorconfig,
/// archives. #992.
pub const ICON_GREY_LIGHT: (u8, u8, u8) = (0x6d, 0x80, 0x86);
/// Seti-UI ignore (`@ignore`) — `.gitignore`/`.gitattributes` and other
/// VCS-ignored-style files. #992.
pub const ICON_IGNORE: (u8, u8, u8) = (0x41, 0x53, 0x5b);

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
        "cs" => ICON_BLUE,
        "tf" | "tfvars" => ICON_PURPLE,
        "java" => ICON_RED,
        "kt" | "kts" => ICON_ORANGE,
        "scala" => ICON_RED,
        "rb" => ICON_RED,
        "php" => ICON_PURPLE,
        "swift" => ICON_ORANGE,
        "dart" => ICON_BLUE,
        "sql" => ICON_PINK,
        "xml" => ICON_ORANGE,
        "ps1" => ICON_BLUE,
        "fs" | "fsx" => ICON_BLUE,
        "ex" | "exs" => ICON_PURPLE,
        "erl" => ICON_RED,
        "hs" => ICON_PURPLE,
        "clj" | "cljs" => ICON_GREEN,
        "nix" => ICON_BLUE,
        "proto" => ICON_CYAN,
        "graphql" | "gql" => ICON_PINK,
        "vue" => ICON_GREEN,
        "svelte" => ICON_RED,
        "scss" | "sass" => ICON_PINK,
        "less" => ICON_BLUE,
        "ini" | "cfg" | "conf" => ICON_GREY_LIGHT,
        "csv" => ICON_GREEN,
        "log" => ICON_NEUTRAL,
        "svg" => ICON_PURPLE,
        "png" | "jpg" | "jpeg" | "gif" | "webp" => ICON_PURPLE,
        "pdf" => ICON_RED,
        "zip" | "tar" | "gz" => ICON_GREY_LIGHT,
        _ => ICON_NEUTRAL,
    }
}

// ─── Filename-Matched Icon Lookup (#992) ────────────────────────────────────
//
// `Path::extension()` returns `None` for `Dockerfile` (no dot at all) *and*
// for `.gitignore` (Rust treats a leading-dot name as having no extension,
// not as an empty-name-plus-extension) — so no arm above can ever badge
// either one. VS Code/Seti-UI badge these by exact (lowercased) filename
// instead, consulted *before* the extension table. See `file_icon_for_name`/
// `file_icon_color_for_name`, the two functions callers should actually use.

/// Filename-keyed icon glyph for the well-known files Seti-UI badges by name
/// rather than extension. `None` means "no filename match — fall back to
/// [`file_icon`]". Pairs 1:1 with [`filename_icon_color`], mirroring the
/// [`file_icon`]/[`file_icon_color`] invariant.
fn filename_icon(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "dockerfile" => Some(FILE_DOCKER.s()),
        "makefile" => Some(FILE_MAKEFILE.s()),
        "cmakelists.txt" => Some(FILE_MAKEFILE.s()),
        "license" => Some(FILE_LICENSE.s()),
        "readme" => Some(FILE_INFO.s()),
        ".gitignore" => Some(FILE_GIT.s()),
        ".gitattributes" => Some(FILE_GIT.s()),
        ".editorconfig" => Some(FILE_CONFIG.s()),
        _ => None,
    }
}

/// Filename-keyed icon colour, pairing 1:1 with [`filename_icon`].
fn filename_icon_color(name: &str) -> Option<(u8, u8, u8)> {
    match name.to_lowercase().as_str() {
        "dockerfile" => Some(ICON_BLUE),
        "makefile" => Some(ICON_ORANGE),
        "cmakelists.txt" => Some(ICON_BLUE),
        "license" => Some(ICON_YELLOW),
        "readme" => Some(ICON_BLUE),
        ".gitignore" => Some(ICON_IGNORE),
        ".gitattributes" => Some(ICON_IGNORE),
        ".editorconfig" => Some(ICON_GREY_LIGHT),
        _ => None,
    }
}

/// The lowercase extension component of `name`, the same way [`file_icon`]
/// expects its argument. `name` may be a bare filename or a path; only the
/// final component's extension is used.
fn extension_of(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Return the icon glyph for a file, given its **base name** (e.g.
/// `"Dockerfile"`, `"main.rs"` — not a full path, though a full path's final
/// component still resolves correctly via [`extension_of`]). Checks the
/// filename table first (`Dockerfile`, `.gitignore`, ...) and falls back to
/// [`file_icon`] on the extension otherwise. This is the function both the
/// tab bar and the explorer tree should call — see `render.rs`'s
/// `build_tab_bar_icons` and `build_explorer_tree_rows`.
pub fn file_icon_for_name(name: &str) -> &'static str {
    filename_icon(name).unwrap_or_else(|| file_icon(&extension_of(name)))
}

/// The colour counterpart to [`file_icon_for_name`]; pairs with it the same
/// way [`file_icon_color`] pairs with [`file_icon`].
pub fn file_icon_color_for_name(name: &str) -> (u8, u8, u8) {
    filename_icon_color(name).unwrap_or_else(|| file_icon_color(&extension_of(name)))
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
    use regex::Regex;

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

    // ─── #992: stop the extension/filename tables silently rotting ────────

    /// `include_str!`-based self-parse of this file's own source, mirroring
    /// `tests/icon_font_coverage.rs`'s approach of parsing `src/icons.rs`
    /// text directly rather than hand-maintaining a second copy of the
    /// arm list that could itself drift from the real one.
    const SELF_SOURCE: &str = include_str!("icons.rs");

    /// Every quoted string-literal key used as a match-arm pattern in the
    /// named function's body (located by finding `fn <name>(` then brace-
    /// matching to the function's closing `}`). This only works because
    /// `file_icon`, `file_icon_color`, `filename_icon` and
    /// `filename_icon_color` have no string literals anywhere in their
    /// bodies except the arm keys themselves -- every `"..."` found is an
    /// arm.
    fn match_arm_keys(fn_name: &str) -> std::collections::BTreeSet<String> {
        let needle = format!("fn {fn_name}(");
        let fn_start = SELF_SOURCE
            .find(&needle)
            .unwrap_or_else(|| panic!("fn {fn_name} not found in icons.rs source"));
        let body_start = SELF_SOURCE[fn_start..]
            .find('{')
            .map(|i| fn_start + i)
            .unwrap_or_else(|| panic!("no fn body found for {fn_name}"));
        let mut depth = 0i32;
        let mut body_end = body_start;
        for (i, ch) in SELF_SOURCE[body_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = body_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        assert!(body_end > body_start, "unbalanced braces for {fn_name}");
        let body = &SELF_SOURCE[body_start..=body_end];
        let re = Regex::new(r#""([a-z0-9.]+)""#).unwrap();
        re.captures_iter(body).map(|c| c[1].to_string()).collect()
    }

    /// `file_icon`'s doc comment (and the #703 design note above
    /// `file_icon_color`) claims "every arm there has an arm here". Nothing
    /// enforced that before this test: a future edit that adds an extension
    /// to one table and forgets the other now fails loudly here instead of
    /// silently shipping a badge with the generic colour (or vice versa).
    #[test]
    fn file_icon_and_file_icon_color_cover_the_same_extensions() {
        let icon_keys = match_arm_keys("file_icon");
        let color_keys = match_arm_keys("file_icon_color");
        assert!(
            !icon_keys.is_empty(),
            "regex found zero arms in file_icon -- the parser almost \
             certainly broke, not that file_icon went empty"
        );
        let only_icon: Vec<_> = icon_keys.difference(&color_keys).collect();
        let only_color: Vec<_> = color_keys.difference(&icon_keys).collect();
        assert!(
            only_icon.is_empty() && only_color.is_empty(),
            "file_icon and file_icon_color must cover exactly the same \
             extensions; only in file_icon: {only_icon:?}, only in \
             file_icon_color: {only_color:?}"
        );
    }

    /// Same invariant for the filename-matched table (#992).
    #[test]
    fn filename_icon_and_filename_icon_color_cover_the_same_filenames() {
        let icon_keys = match_arm_keys("filename_icon");
        let color_keys = match_arm_keys("filename_icon_color");
        assert!(
            !icon_keys.is_empty(),
            "regex found zero arms in filename_icon -- the parser almost \
             certainly broke, not that filename_icon went empty"
        );
        assert_eq!(
            icon_keys, color_keys,
            "filename_icon and filename_icon_color must cover exactly the \
             same filenames"
        );
    }

    /// #992: a fixed, human-readable list of extensions that must resolve
    /// to a non-generic icon. This is what actually catches the original
    /// bug report -- `.cs` and `.tf` silently falling through to
    /// `FILE_GENERIC` because no arm existed for them -- in a way the
    /// invariant tests above cannot (they only check the two tables agree
    /// with *each other*, not that either one covers any given extension).
    #[test]
    fn representative_extensions_get_non_generic_icons() {
        let exts = [
            "cs", "tf", "tfvars", "java", "kt", "kts", "scala", "rb", "php", "swift", "dart",
            "sql", "xml", "ps1", "fs", "fsx", "ex", "exs", "erl", "hs", "clj", "cljs", "nix",
            "proto", "graphql", "gql", "vue", "svelte", "scss", "sass", "less", "ini", "cfg",
            "conf", "csv", "svg", "png", "jpg", "jpeg", "gif", "webp", "pdf", "zip", "tar", "gz",
        ];
        for ext in exts {
            assert_ne!(
                file_icon(ext),
                FILE_GENERIC.s(),
                "extension {ext:?} must not fall through to the generic icon"
            );
        }
    }

    /// #992 deliverable 2: filename-badged files resolve to a non-generic
    /// icon via [`file_icon_for_name`] -- the function callers should
    /// actually use (bare [`file_icon`] cannot see filenames at all; see the
    /// dotfile regression test below).
    #[test]
    fn filename_matched_files_get_non_generic_icons() {
        let names = [
            "Dockerfile",
            "dockerfile",
            "Makefile",
            "CMakeLists.txt",
            "LICENSE",
            "README",
            ".gitignore",
            ".gitattributes",
            ".editorconfig",
        ];
        for name in names {
            assert_ne!(
                file_icon_for_name(name),
                FILE_GENERIC.s(),
                "filename {name:?} must not fall through to the generic icon"
            );
        }
    }

    /// #992, the actual regression this deliverable fixes: `Path::extension()`
    /// is `None` for a leading-dot filename like `.gitignore` (Rust does not
    /// treat a leading dot as separating an empty name from an extension),
    /// so `file_icon("")` -- what a pre-#992 call site resolves a dotfile to
    /// -- always returns the generic icon regardless of any extension-table
    /// arm. `file_icon_for_name` must consult the filename table before
    /// falling through to that empty-extension lookup.
    #[test]
    fn dotfile_extension_is_empty_but_file_icon_for_name_still_resolves() {
        assert_eq!(extension_of(".gitignore"), "");
        assert_ne!(file_icon_for_name(".gitignore"), file_icon(""));
    }

    /// `Cargo.toml`/`package.json` are named in the issue as VS Code
    /// filename-badges, but Seti-UI (the palette/glyph source this table
    /// draws from) has no dedicated entry for either -- so `filename_icon`
    /// intentionally has no arm for them, and they fall through to their
    /// already-non-generic extension arms (`toml`/`json`). Pin the fall-
    /// through so a future edit doesn't "fix" it into a redundant special
    /// case.
    #[test]
    fn cargo_toml_and_package_json_fall_through_to_extension_icons() {
        assert_eq!(file_icon_for_name("Cargo.toml"), file_icon("toml"));
        assert_eq!(file_icon_for_name("package.json"), file_icon("json"));
    }
}
