use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::ai::AiMessage;
use super::buffer::{Buffer, BufferId};
use super::buffer_manager::{BufferManager, BufferState, UndoEntry};
use super::comment;
use super::dap::{BreakpointInfo, DapEvent, DapVariable, StackFrame};
use super::dap_manager::{
    generate_launch_json, parse_launch_json, parse_tasks_json, task_to_shell_command,
    type_to_adapter, DapManager, LaunchConfig,
};
use super::extensions;
use super::git;
use super::lsp::{
    self, Diagnostic, DiagnosticSeverity, FormattingEdit, LspEvent, SignatureHelpData,
    WorkspaceEdit,
};
use super::lsp_manager::LspManager;
use super::paths;
use super::plugin;
use super::project_search::{self, ProjectMatch, ReplaceResult, SearchError, SearchOptions};
use super::registry;
use super::session::{ExtensionState, HistoryState, SessionGroupLayout, SessionState};
use super::settings::{EditorMode, Settings};
use super::syntax::Syntax;
use super::tab::{Tab, TabId};
use super::terminal::{default_shell, InstallContext, TerminalPane};
use super::view::{FoldRegion, View};
use super::window::{
    DropZone, GroupDivider, GroupId, GroupLayout, SplitDirection, Window, WindowId, WindowLayout,
    WindowRect,
};
use std::borrow::Cow;

use super::{Cursor, Mode};

/// High bit marker for synthetic "Non-Public Members" group var_refs.
/// Real DAP adapters use sequential integers that never set this bit.
const SYNTHETIC_NON_PUBLIC_MASK: u64 = 0x8000_0000_0000_0000;

/// Actions returned from `handle_key` that the UI layer must act on.
/// This keeps GTK/platform concerns out of the core engine.
#[derive(Debug, PartialEq)]
pub enum EngineAction {
    None,
    Quit,
    SaveQuit,
    OpenFile(PathBuf),
    /// Display an error to the user (engine already set self.message)
    Error,
    /// Open the integrated terminal panel (UI layer provides correct cols/rows)
    OpenTerminal,
    /// Run a command in a visible terminal pane (UI layer provides cols/rows).
    /// The string is the shell command to execute.
    RunInTerminal(String),
    /// Open Folder dialog requested (UI layer shows the native picker)
    OpenFolderDialog,
    /// Open Workspace dialog requested (UI layer shows the native picker)
    OpenWorkspaceDialog,
    /// Save Workspace As dialog requested (UI layer shows the native picker)
    SaveWorkspaceAsDialog,
    /// Open Recent workspaces dialog requested (UI layer shows picker)
    OpenRecentDialog,
    /// There are unsaved buffers; the UI must ask the user to confirm before quitting.
    QuitWithUnsaved,
    /// Toggle the file-explorer sidebar (UI layer handles show/hide).
    ToggleSidebar,
    /// Quit with non-zero exit code (:cquit).
    QuitWithError,
    /// Open a URL in the default browser (validated as safe by the engine).
    OpenUrl(String),
}

/// A row in the Settings sidebar flat list.
/// Distinguishes core settings from extension-declared settings.
#[derive(Debug, Clone)]
pub enum SettingsRow {
    /// Core category header: index into `setting_categories()`.
    CoreCategory(usize),
    /// Core setting: index into `SETTING_DEFS`.
    CoreSetting(usize),
    /// Extension category header: extension name.
    ExtCategory(String),
    /// Extension setting: `(extension_name, setting_key)`.
    ExtSetting(String, String),
}

// ── Ex command abbreviation table ────────────────────────────────────────────
// Each entry is (canonical_name, min_prefix_length).
// `normalize_ex_command("sor foo")` → `"sort foo"`.
static EX_ABBREVS: &[(&str, usize)] = &[
    ("bdelete", 2),
    ("bnext", 2),
    ("bprevious", 2),
    ("buffer", 1),
    ("cclose", 3),
    ("close", 3),
    ("cnext", 2),
    ("colorscheme", 4),
    ("copen", 4),
    ("copy", 2),
    ("cprevious", 2),
    ("cquit", 2),
    ("delete", 1),
    ("display", 2),
    ("echo", 2),
    ("edit", 1),
    ("enew", 3),
    ("file", 1),
    ("grep", 2),
    ("help", 1),
    ("history", 3),
    ("join", 1),
    ("jumps", 2),
    ("make", 3),
    ("mark", 2),
    ("move", 1),
    ("nohlsearch", 3),
    ("number", 2),
    ("only", 2),
    ("print", 1),
    ("put", 2),
    ("pwd", 2),
    ("qall", 2),
    ("quit", 1),
    ("read", 1),
    ("redo", 3),
    ("registers", 3),
    ("retab", 3),
    ("saveas", 3),
    ("set", 2),
    ("sort", 3),
    ("split", 2),
    ("tabclose", 4),
    ("tabmove", 4),
    ("tabnext", 4),
    ("tabprevious", 4),
    ("terminal", 2),
    ("undo", 1),
    ("update", 2),
    ("version", 2),
    ("vimgrep", 3),
    ("vnew", 3),
    ("vsplit", 2),
    ("wall", 2),
    ("wincmd", 4),
    ("wqall", 3),
    ("write", 1),
    ("xall", 2),
    ("yank", 1),
];

/// Split an ex-command string into (command_word, bang, rest).
/// Example: `"q!"` → `("q", "!", "")`, `"sor foo"` → `("sor", "", " foo")`
fn split_ex_command(input: &str) -> (&str, &str, &str) {
    // Find end of alphabetic command word
    let cmd_end = input
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(input.len());
    let cmd_word = &input[..cmd_end];
    let rest = &input[cmd_end..];
    if let Some(after_bang) = rest.strip_prefix('!') {
        (cmd_word, "!", after_bang)
    } else {
        (cmd_word, "", rest)
    }
}

/// Normalize an ex-command abbreviation to its canonical form.
/// Returns `Cow::Borrowed` when no transformation is needed.
pub fn normalize_ex_command(input: &str) -> Cow<'_, str> {
    if input.is_empty() {
        return Cow::Borrowed(input);
    }
    let first = input.as_bytes()[0];
    // Skip: uppercase-starting (VimCode commands), s/, g/, v/, !, digit-starting, #
    if first.is_ascii_uppercase()
        || input.starts_with("s/")
        || input.starts_with("g/")
        || input.starts_with("v/")
        || first == b'!'
        || first.is_ascii_digit()
        || first == b'#'
    {
        return Cow::Borrowed(input);
    }

    let (cmd_word, bang, rest) = split_ex_command(input);
    if cmd_word.is_empty() {
        return Cow::Borrowed(input);
    }

    // Look up in abbreviation table
    for &(canonical, min_len) in EX_ABBREVS {
        if cmd_word.len() >= min_len
            && cmd_word.len() <= canonical.len()
            && canonical.starts_with(cmd_word)
        {
            if cmd_word == canonical {
                // Already canonical
                return Cow::Borrowed(input);
            }
            return Cow::Owned(format!("{}{}{}", canonical, bang, rest));
        }
    }

    Cow::Borrowed(input)
}

/// Pending swap file recovery state — the user must press R/D/A.
pub struct SwapRecovery {
    pub swap_path: PathBuf,
    pub recovered_content: String,
    pub buffer_id: BufferId,
}

/// A button in a modal dialog.
#[derive(Debug, Clone)]
pub struct DialogButton {
    pub label: String,
    pub hotkey: char,
    pub action: String,
}

/// A modal dialog displayed over the editor.
#[derive(Debug, Clone)]
pub struct Dialog {
    pub title: String,
    pub body: Vec<String>,
    pub buttons: Vec<DialogButton>,
    pub selected: usize,
    pub tag: String,
    /// Optional text input field (e.g. for SSH passphrase prompts).
    /// When `Some`, the dialog shows an editable input line.
    pub input: Option<DialogInput>,
}

/// Text input state for a dialog with an editable field.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DialogInput {
    pub label: String,
    pub value: String,
    pub is_password: bool,
}

/// A single entry in the command palette.
pub struct PaletteCommand {
    pub label: &'static str,
    /// Shortcut displayed in Vim mode (may be a Vim key like "gd" or "u").
    pub shortcut: &'static str,
    /// Shortcut displayed in VSCode mode; empty means fall back to `shortcut`.
    pub vscode_shortcut: &'static str,
    pub action: &'static str,
}

/// All commands available in the command palette.
pub static PALETTE_COMMANDS: &[PaletteCommand] = &[
    // File
    PaletteCommand {
        label: "File: New Tab",
        shortcut: "Ctrl+T",
        vscode_shortcut: "",
        action: "tabnew",
    },
    PaletteCommand {
        label: "File: Open File…",
        shortcut: "",
        vscode_shortcut: "",
        action: "open_file_dialog",
    },
    PaletteCommand {
        label: "File: Open Folder…",
        shortcut: "",
        vscode_shortcut: "",
        action: "open_folder_dialog",
    },
    PaletteCommand {
        label: "File: Open Workspace From File…",
        shortcut: "",
        vscode_shortcut: "",
        action: "open_workspace_dialog",
    },
    PaletteCommand {
        label: "File: Save Workspace As…",
        shortcut: "",
        vscode_shortcut: "",
        action: "save_workspace_as_dialog",
    },
    PaletteCommand {
        label: "File: Open Recent…",
        shortcut: "",
        vscode_shortcut: "",
        action: "openrecent",
    },
    PaletteCommand {
        label: "File: Save",
        shortcut: "Ctrl+S",
        vscode_shortcut: "",
        action: "w",
    },
    PaletteCommand {
        label: "File: Save As…",
        shortcut: "",
        vscode_shortcut: "",
        action: "saveas",
    },
    PaletteCommand {
        label: "File: Close Tab",
        shortcut: "",
        vscode_shortcut: "Ctrl+W",
        action: "tabclose",
    },
    PaletteCommand {
        label: "File: Quit",
        shortcut: "",
        vscode_shortcut: "",
        action: "q",
    },
    // Edit
    PaletteCommand {
        label: "Edit: Undo",
        shortcut: "u",
        vscode_shortcut: "Ctrl+Z",
        action: "undo",
    },
    PaletteCommand {
        label: "Edit: Redo",
        shortcut: "Ctrl+R",
        vscode_shortcut: "Ctrl+Y",
        action: "redo",
    },
    PaletteCommand {
        label: "Edit: Find & Replace",
        shortcut: "",
        vscode_shortcut: "Ctrl+H",
        action: "substitute",
    },
    // View
    PaletteCommand {
        label: "View: Toggle Sidebar",
        shortcut: "Ctrl+B",
        vscode_shortcut: "",
        action: "sidebar",
    },
    PaletteCommand {
        label: "View: Toggle Terminal",
        shortcut: "Ctrl+T",
        vscode_shortcut: "",
        action: "term",
    },
    PaletteCommand {
        label: "View: Toggle Menu Bar",
        shortcut: "",
        vscode_shortcut: "",
        action: "togglemenu",
    },
    PaletteCommand {
        label: "View: Zoom In",
        shortcut: "Ctrl++",
        vscode_shortcut: "",
        action: "zoomin",
    },
    PaletteCommand {
        label: "View: Zoom Out",
        shortcut: "Ctrl+-",
        vscode_shortcut: "",
        action: "zoomout",
    },
    PaletteCommand {
        label: "View: Command Palette",
        shortcut: "F1",
        vscode_shortcut: "F1",
        action: "palette",
    },
    // Go
    PaletteCommand {
        label: "Go: Find File (Fuzzy)",
        shortcut: "Ctrl+P",
        vscode_shortcut: "",
        action: "fuzzy",
    },
    PaletteCommand {
        label: "Go: Command Center",
        shortcut: "",
        vscode_shortcut: "",
        action: "CommandCenter",
    },
    PaletteCommand {
        label: "Go: Live Grep",
        shortcut: "Ctrl+G",
        vscode_shortcut: "Ctrl+Shift+F",
        action: "grep",
    },
    PaletteCommand {
        label: "Search: Word Under Cursor",
        shortcut: "<leader>sw",
        vscode_shortcut: "<leader>sw",
        action: "GrepWord",
    },
    PaletteCommand {
        label: "Search: Open Buffers",
        shortcut: "<leader>sb",
        vscode_shortcut: "<leader>sb",
        action: "Buffers",
    },
    PaletteCommand {
        label: "Go to Symbol in Editor (Outline)",
        shortcut: "<leader>so",
        vscode_shortcut: "<leader>so",
        action: "document_outline",
    },
    PaletteCommand {
        label: "Help: Search Key Bindings",
        shortcut: "<leader>sk",
        vscode_shortcut: "<leader>sk",
        action: "search_keybindings",
    },
    PaletteCommand {
        label: "Go: Go to Line",
        shortcut: "",
        vscode_shortcut: "Ctrl+G",
        action: "goto_line",
    },
    PaletteCommand {
        label: "Go: Go to Definition",
        shortcut: "gd",
        vscode_shortcut: "F12",
        action: "lsp_definition",
    },
    PaletteCommand {
        label: "Go: Go to References",
        shortcut: "gr",
        vscode_shortcut: "Shift+F12",
        action: "lsp_references",
    },
    PaletteCommand {
        label: "Go: Go to Implementation",
        shortcut: "gi",
        vscode_shortcut: "Ctrl+F12",
        action: "LspImpl",
    },
    PaletteCommand {
        label: "Go: Jump Back",
        shortcut: "Ctrl+O",
        vscode_shortcut: "Alt+Left",
        action: "jump_back",
    },
    PaletteCommand {
        label: "Go: Navigate Back",
        shortcut: "Ctrl+Alt+Left",
        vscode_shortcut: "",
        action: "navback",
    },
    PaletteCommand {
        label: "Go: Navigate Forward",
        shortcut: "Ctrl+Alt+Right",
        vscode_shortcut: "",
        action: "navforward",
    },
    // Run / Debug
    PaletteCommand {
        label: "Debug: Start / Continue",
        shortcut: "F5",
        vscode_shortcut: "",
        action: "debug",
    },
    PaletteCommand {
        label: "Debug: Pause",
        shortcut: "F6",
        vscode_shortcut: "",
        action: "pause",
    },
    PaletteCommand {
        label: "Debug: Stop",
        shortcut: "Shift+F5",
        vscode_shortcut: "",
        action: "stop",
    },
    PaletteCommand {
        label: "Debug: Step Over",
        shortcut: "F10",
        vscode_shortcut: "",
        action: "stepover",
    },
    PaletteCommand {
        label: "Debug: Step Into",
        shortcut: "F11",
        vscode_shortcut: "",
        action: "stepin",
    },
    PaletteCommand {
        label: "Debug: Step Out",
        shortcut: "Shift+F11",
        vscode_shortcut: "",
        action: "stepout",
    },
    PaletteCommand {
        label: "Debug: Toggle Breakpoint",
        shortcut: "F9",
        vscode_shortcut: "",
        action: "togglebp",
    },
    PaletteCommand {
        label: "Debug: Install Adapter",
        shortcut: "",
        vscode_shortcut: "",
        action: "DapInstall",
    },
    // Terminal
    PaletteCommand {
        label: "Terminal: New Terminal",
        shortcut: "",
        vscode_shortcut: "",
        action: "term",
    },
    PaletteCommand {
        label: "Terminal: Close Terminal",
        shortcut: "",
        vscode_shortcut: "",
        action: "termclose",
    },
    // Git
    PaletteCommand {
        label: "Git: Status",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gstatus",
    },
    PaletteCommand {
        label: "Git: Diff",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gdiff",
    },
    PaletteCommand {
        label: "Git: Diff Split",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gdiffsplit",
    },
    PaletteCommand {
        label: "Git: Switch Branch",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gbranches",
    },
    PaletteCommand {
        label: "Git: Create Branch",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gbranch",
    },
    PaletteCommand {
        label: "Git: Blame",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gblame",
    },
    PaletteCommand {
        label: "Git: Stage Hunk",
        shortcut: "gs",
        vscode_shortcut: "gs",
        action: "Ghs",
    },
    PaletteCommand {
        label: "Git: Push",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gpush",
    },
    PaletteCommand {
        label: "Git: Pull",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gpull",
    },
    PaletteCommand {
        label: "Git: Fetch",
        shortcut: "",
        vscode_shortcut: "",
        action: "Gfetch",
    },
    PaletteCommand {
        label: "Git: Peek Change",
        shortcut: "gD",
        vscode_shortcut: "gD",
        action: "DiffPeek",
    },
    PaletteCommand {
        label: "Git: Toggle Inline Blame",
        shortcut: "<leader>gb",
        vscode_shortcut: "",
        action: "ToggleBlame",
    },
    // LSP
    PaletteCommand {
        label: "LSP: Info",
        shortcut: "",
        vscode_shortcut: "",
        action: "LspInfo",
    },
    PaletteCommand {
        label: "LSP: Restart",
        shortcut: "",
        vscode_shortcut: "",
        action: "LspRestart",
    },
    PaletteCommand {
        label: "LSP: Code Action",
        shortcut: "<leader>ca",
        vscode_shortcut: "Ctrl+.",
        action: "CodeAction",
    },
    PaletteCommand {
        label: "LSP: Format Document",
        shortcut: "",
        vscode_shortcut: "Shift+Alt+F",
        action: "Lformat",
    },
    PaletteCommand {
        label: "LSP: Rename Symbol",
        shortcut: "",
        vscode_shortcut: "F2",
        action: "Rename",
    },
    PaletteCommand {
        label: "LSP: Install Server",
        shortcut: "",
        vscode_shortcut: "",
        action: "LspInstall",
    },
    // Settings
    PaletteCommand {
        label: "Settings: Toggle Wrap",
        shortcut: "",
        vscode_shortcut: "",
        action: "set_wrap_toggle",
    },
    PaletteCommand {
        label: "Settings: Toggle Line Numbers",
        shortcut: "",
        vscode_shortcut: "",
        action: "set_number_toggle",
    },
    PaletteCommand {
        label: "Settings: Toggle Relative Numbers",
        shortcut: "",
        vscode_shortcut: "",
        action: "set_rnu_toggle",
    },
    PaletteCommand {
        label: "Settings: Plugin List",
        shortcut: "",
        vscode_shortcut: "",
        action: "Plugin list",
    },
    PaletteCommand {
        label: "Preferences: Open Settings (JSON)",
        shortcut: "",
        vscode_shortcut: "",
        action: "Settings",
    },
    // Editor groups
    PaletteCommand {
        label: "View: Split Editor Right",
        shortcut: "Ctrl+\\",
        vscode_shortcut: "Ctrl+\\",
        action: "EditorGroupSplit",
    },
    PaletteCommand {
        label: "View: Split Editor Down",
        shortcut: "Ctrl-W E",
        vscode_shortcut: "",
        action: "EditorGroupSplitDown",
    },
    PaletteCommand {
        label: "View: Close Editor Group",
        shortcut: "",
        vscode_shortcut: "",
        action: "EditorGroupClose",
    },
    PaletteCommand {
        label: "View: Focus Other Group",
        shortcut: "Ctrl+2",
        vscode_shortcut: "Ctrl+2",
        action: "EditorGroupFocus",
    },
    PaletteCommand {
        label: "View: Move Tab to Other Group",
        shortcut: "",
        vscode_shortcut: "",
        action: "EditorGroupMoveTab",
    },
    PaletteCommand {
        label: "Markdown: Preview Side-by-Side",
        shortcut: "",
        vscode_shortcut: "",
        action: "MarkdownPreview",
    },
    PaletteCommand {
        label: "Preferences: Open Keybinding Reference",
        shortcut: "",
        vscode_shortcut: "",
        action: "Keybindings",
    },
    PaletteCommand {
        label: "Preferences: Open Keyboard Shortcuts",
        shortcut: "",
        vscode_shortcut: "",
        action: "Keymaps",
    },
    PaletteCommand {
        label: "Diff: Next Change",
        shortcut: "]c",
        vscode_shortcut: "",
        action: "DiffNext",
    },
    PaletteCommand {
        label: "Diff: Previous Change",
        shortcut: "[c",
        vscode_shortcut: "",
        action: "DiffPrev",
    },
    PaletteCommand {
        label: "Diff: Toggle Hide Unchanged",
        shortcut: "",
        vscode_shortcut: "",
        action: "DiffToggleContext",
    },
    // Spell checking
    PaletteCommand {
        label: "Toggle Spell Check",
        shortcut: ":set spell",
        vscode_shortcut: "",
        action: "toggle_spell",
    },
];

// ─── Unified Picker Types ────────────────────────────────────────────────────

/// Identifies the data source backing a picker modal.
/// Action triggered when a status bar segment is clicked.
#[derive(Debug, Clone, PartialEq)]
pub enum StatusAction {
    GoToLine,
    ChangeLanguage,
    ChangeIndentation,
    ChangeLineEnding,
    ChangeEncoding,
    SwitchBranch,
    LspInfo,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum PickerSource {
    Files,
    Grep,
    Commands,
    Buffers,
    Keybindings,
    RecentFiles,
    Marks,
    Registers,
    GitBranches,
    /// Command Center: dynamic prefix routing (>, @, #, :, ?).
    CommandCenter,
    /// Language/filetype picker (click on language segment in status bar).
    Languages,
    /// Indentation picker (click on indent segment in status bar).
    Indentation,
    /// Line ending picker (LF / CRLF).
    LineEndings,
    Custom(String),
}

/// Cached breadcrumb segment info for keyboard navigation.
/// Mirrors render::BreadcrumbSegment but lives in the engine (library) crate.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BreadcrumbSegmentInfo {
    pub label: String,
    pub is_symbol: bool,
    pub path_prefix: Option<PathBuf>,
    pub symbol_line: Option<usize>,
    /// For symbol segments: the name of the parent scope (container).
    /// `None` for top-level symbols and path segments.
    pub parent_scope: Option<String>,
}

/// A single item in the picker list.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PickerItem {
    /// Text shown in the result list.
    pub display: String,
    /// Text matched against the query by `fuzzy_score`.
    pub filter_text: String,
    /// Right-aligned hint (shortcut, line number, etc.).
    pub detail: Option<String>,
    /// What happens when this item is confirmed.
    pub action: PickerAction,
    /// Nerd Font icon prefix.
    pub icon: Option<String>,
    /// Fuzzy match score (set during filtering).
    pub score: i32,
    /// Byte positions in `display` that matched the query (for highlight).
    pub match_positions: Vec<usize>,
    /// Tree nesting depth (0 = top-level). Used for symbol outline tree view.
    pub depth: usize,
    /// Whether this item has children that can be expanded.
    pub expandable: bool,
    /// Whether this item's children are currently visible.
    pub expanded: bool,
}

/// The action taken when a picker item is confirmed.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PickerAction {
    OpenFile(PathBuf),
    OpenFileAtLine(PathBuf, usize),
    ExecuteCommand(String),
    JumpToMark(char),
    PasteRegister(char),
    CheckoutBranch(String),
    /// Go to a specific line number in the active buffer.
    GotoLine(usize),
    /// Jump to a symbol location (file, line, col).
    GotoSymbol(PathBuf, usize, usize),
    /// Set language/filetype for the active buffer.
    SetLanguage(String),
    /// Set indentation: (expand_tab, tabstop/shift_width).
    SetIndentation(bool, u8),
    /// Set line ending format: true = CRLF, false = LF.
    SetLineEnding(bool),
    Custom(String),
}

/// Preview context shown in the picker's right pane.
#[derive(Debug, Clone)]
pub struct PickerPreview {
    /// Lines to display: (1-based line number, text, is_highlighted).
    pub lines: Vec<(usize, String, bool)>,
}

/// How a file should be opened: as a temporary preview or permanent buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenMode {
    /// Preview mode: replaces the current window's buffer temporarily.
    /// Used internally; sidebar clicks use `open_file_in_tab` instead.
    #[allow(dead_code)]
    Preview,
    Permanent,
}

/// Maximum depth for macro recursion to prevent infinite loops.
const MAX_MACRO_RECURSION: usize = 100;

/// Number of context lines to keep visible around diff changes when hiding unchanged sections.
const DIFF_CONTEXT_LINES: usize = 3;

/// Per-line diff status used by the two-way diff feature (`:diffthis` / `:diffsplit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLine {
    Same,
    Added,
    Removed,
    /// Filler line inserted for alignment — no buffer content.
    Padding,
}

/// One entry in the aligned diff sequence.  Maps a visual row to either
/// a real buffer line (`source_line = Some(n)`) or a padding filler
/// (`source_line = None`).
#[derive(Clone, Copy, Debug)]
pub struct AlignedDiffEntry {
    pub source_line: Option<usize>,
}

/// Direction of the last search operation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchDirection {
    Forward,  // Last search was '/'
    Backward, // Last search was '?'
}

/// Represents a change operation that can be repeated with `.`
#[derive(Debug, Clone)]
struct Change {
    /// Type of operation
    op: ChangeOp,
    /// Text inserted (for insert operations)
    text: String,
    /// Count used with the operation
    count: usize,
    /// Motion used with operator (for d/c with motions)
    motion: Option<Motion>,
}

/// C preprocessor directive kind for `[#` / `]#` navigation.
pub(crate) enum PreprocKind {
    If,
    ElseElif,
    Endif,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ChangeOp {
    Insert,
    Delete,
    Change,
    Substitute,
    SubstituteLine,
    DeleteToEnd,
    ChangeToEnd,
    Replace,
    ToggleCase,
    Join,
    Indent,
    Dedent,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    WordBackwardEnd,
    LineStart,
    LineEnd,
    DeleteLine,
    CharFind(char, char), // (motion_type, target_char)
    ParagraphForward,
    ParagraphBackward,
    MatchingBracket,
    TextObject(char, char), // (modifier, object) - e.g., ('i', 'w')
}

/// Which section of the debug sidebar currently has the selection cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugSidebarSection {
    #[default]
    Variables,
    Watch,
    CallStack,
    Breakpoints,
}

/// Which panel is shown in the bottom area (Terminal or Debug Output).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BottomPanelKind {
    #[default]
    Terminal,
    DebugOutput,
}

/// State of an in-progress tab drag operation.
#[derive(Debug, Clone)]
pub struct TabDragState {
    pub source_group: GroupId,
    pub source_tab_index: usize,
    pub tab_name: String,
}

// ── Context menu data model ──────────────────────────────────────────────────

/// What the context menu was opened on.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ContextMenuTarget {
    Tab { group_id: GroupId, tab_idx: usize },
    ExplorerFile { path: PathBuf },
    ExplorerDir { path: PathBuf },
    Editor,
    EditorActionMenu { group_id: GroupId },
    ExtPanel { panel_name: String, item_id: String },
}

/// A single item in a context menu popup.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ContextMenuItem {
    pub label: String,
    pub action: String,
    pub shortcut: String,
    pub separator_after: bool,
    pub enabled: bool,
}

/// State for a sidebar hover popup with rendered markdown content.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PanelHoverPopup {
    /// Rendered markdown lines + spans (reuses the existing markdown module).
    pub rendered: crate::core::markdown::MdRendered,
    /// Clickable link URLs extracted from LinkUrl spans: (line_idx, start_byte, end_byte, url).
    pub links: Vec<(usize, usize, usize, String)>,
    /// Panel name this hover belongs to.
    pub panel_name: String,
    /// Item ID within the panel.
    pub item_id: String,
    /// Flat index of the hovered item (used for positioning).
    pub item_index: usize,
}

impl PanelHoverPopup {
    /// Whether this hover comes from a native (trusted) panel like source_control.
    pub fn is_native(&self) -> bool {
        self.panel_name == "source_control"
    }
}

/// Source of an editor hover popup.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum EditorHoverSource {
    /// LSP hover information (triggered by K or gh).
    Lsp,
    /// LSP diagnostic message at position.
    Diagnostic,
    /// Plugin annotation virtual text.
    Annotation,
    /// Plugin-registered hover provider.
    Plugin(String),
}

/// State for a hover popup anchored to a position in the editor buffer.
/// Supports rich markdown content, clickable links, and keyboard focus.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EditorHoverPopup {
    /// Rendered markdown content.
    pub rendered: crate::core::markdown::MdRendered,
    /// Clickable link regions: (line_idx, start_byte, end_byte, url).
    pub links: Vec<(usize, usize, usize, String)>,
    /// Buffer line where the hover is anchored (0-indexed).
    pub anchor_line: usize,
    /// Buffer column where the hover is anchored (0-indexed).
    pub anchor_col: usize,
    /// What triggered this hover.
    pub source: EditorHoverSource,
    /// Scroll offset for long content.
    pub scroll_top: usize,
    /// Currently focused link index (for keyboard navigation within the popup).
    pub focused_link: Option<usize>,
    /// Fixed popup width in characters, computed once when first shown.
    pub popup_width: usize,
    /// Frozen scroll offsets — captured when popup is first shown so it stays
    /// at a fixed screen position regardless of subsequent editor scrolling.
    pub frozen_scroll_top: usize,
    pub frozen_scroll_left: usize,
    /// Text selection within the popup content (mouse drag or keyboard).
    pub selection: Option<HoverSelection>,
}

/// Text selection within a hover popup, in content coordinates (line, char column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverSelection {
    /// Anchor position (where selection started).
    pub anchor_line: usize,
    pub anchor_col: usize,
    /// Active position (where selection currently extends to).
    pub active_line: usize,
    pub active_col: usize,
}

impl HoverSelection {
    /// Return the selection range normalized so start <= end.
    pub fn normalized(&self) -> (usize, usize, usize, usize) {
        if (self.anchor_line, self.anchor_col) <= (self.active_line, self.active_col) {
            (
                self.anchor_line,
                self.anchor_col,
                self.active_line,
                self.active_col,
            )
        } else {
            (
                self.active_line,
                self.active_col,
                self.anchor_line,
                self.anchor_col,
            )
        }
    }

    /// Extract the selected text from the given popup lines.
    pub fn extract_text(&self, lines: &[String]) -> String {
        let (sl, sc, el, ec) = self.normalized();
        if sl == el {
            // Single line selection
            if let Some(line) = lines.get(sl) {
                let chars: Vec<char> = line.chars().collect();
                let start = sc.min(chars.len());
                let end = ec.min(chars.len());
                return chars[start..end].iter().collect();
            }
            return String::new();
        }
        let mut result = String::new();
        for li in sl..=el.min(lines.len().saturating_sub(1)) {
            if let Some(line) = lines.get(li) {
                let chars: Vec<char> = line.chars().collect();
                if li == sl {
                    let start = sc.min(chars.len());
                    result.push_str(&chars[start..].iter().collect::<String>());
                } else if li == el {
                    let end = ec.min(chars.len());
                    result.push_str(&chars[..end].iter().collect::<String>());
                } else {
                    result.push_str(line);
                }
                if li < el {
                    result.push('\n');
                }
            }
        }
        result
    }
}

/// Check if a URL has a safe scheme for opening in a browser.
/// Allows `https://`, `http://`, and `command:` schemes.
pub fn is_safe_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("command:")
}

/// Convert a hex ASCII byte to its numeric value (0–15), or `None`.
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// State for an open context menu popup (engine-driven, rendered by TUI).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ContextMenuState {
    pub target: ContextMenuTarget,
    pub items: Vec<ContextMenuItem>,
    pub selected: usize,
    pub screen_x: u16,
    pub screen_y: u16,
}

/// State for inline rename editing in the explorer sidebar.
#[derive(Debug, Clone)]
pub struct ExplorerRenameState {
    /// The file/directory being renamed.
    pub path: PathBuf,
    /// Current text input (the new name).
    pub input: String,
    /// Byte-offset cursor position within `input`.
    pub cursor: usize,
    /// Byte-offset selection anchor (if different from cursor, text between
    /// `selection_anchor` and `cursor` is selected).  `None` = no selection.
    pub selection_anchor: Option<usize>,
}

/// State for inline new-file/folder creation in the explorer sidebar.
#[derive(Debug, Clone)]
pub struct ExplorerNewEntryState {
    /// The directory under which the new entry will be created.
    pub parent_dir: PathBuf,
    /// Current text input (the new name).
    pub input: String,
    /// Byte-offset cursor position within `input`.
    pub cursor: usize,
    /// Whether creating a folder (true) or a file (false).
    pub is_folder: bool,
}

/// One panel in a VSCode-style editor-group split.
/// Each group owns its own tab bar and independent tab navigation.
/// Single-group mode (the default) is identical to the previous behaviour.
#[derive(Debug, Clone)]
pub struct EditorGroup {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    /// Index of the first visible tab in the tab bar (for scroll-into-view).
    /// Updated by `ensure_active_tab_visible()` whenever the active tab changes.
    pub tab_scroll_offset: usize,
    /// Available width of the tab bar in character columns.  Set by the
    /// renderer each frame via `Engine::set_tab_bar_width()`.  Defaults to
    /// `usize::MAX` so that before the first render, we assume all tabs fit.
    pub tab_bar_width: usize,
}

impl EditorGroup {
    pub fn new(initial_tab: Tab) -> Self {
        Self {
            tabs: vec![initial_tab],
            active_tab: 0,
            tab_scroll_offset: 0,
            tab_bar_width: usize::MAX,
        }
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    pub fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }
}

// ─── User keymaps ────────────────────────────────────────────────────────────

/// A parsed user-defined key mapping from settings.json.
#[derive(Debug, Clone)]
pub struct UserKeymap {
    /// Mode: "n", "v", "i", "c".
    pub mode: String,
    /// Parsed key sequence, e.g. `["g", "c", "c"]` or `["<C-/>"]`.
    pub keys: Vec<String>,
    /// The ex command to run (without leading `:`), e.g. `"Commentary"`.
    pub action: String,
}

/// Parse a key notation string into individual key specs.
/// `"gcc"` → `["g", "c", "c"]`; `"<C-/>x"` → `["<C-/>", "x"]`.
fn parse_key_sequence(s: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            // Find matching '>'
            if let Some(end) = chars[i..].iter().position(|&c| c == '>') {
                let token: String = chars[i..=i + end].iter().collect();
                keys.push(token);
                i += end + 1;
            } else {
                keys.push(chars[i].to_string());
                i += 1;
            }
        } else {
            keys.push(chars[i].to_string());
            i += 1;
        }
    }
    keys
}

/// Parse a keymap definition string: `"n gcc :Commentary"` → `UserKeymap`.
fn parse_keymap_def(s: &str) -> Option<UserKeymap> {
    let s = s.trim();
    // Split: mode (first char or token), keys, :action
    let mut parts = s.splitn(3, ' ');
    let mode = parts.next()?.to_string();
    if !matches!(mode.as_str(), "n" | "v" | "i" | "c") {
        return None;
    }
    let keys_str = parts.next()?;
    let action_str = parts.next()?.trim();
    if !action_str.starts_with(':') {
        return None;
    }
    let action = action_str[1..].to_string();
    if action.is_empty() {
        return None;
    }
    let keys = parse_key_sequence(keys_str);
    if keys.is_empty() {
        return None;
    }
    Some(UserKeymap { mode, keys, action })
}

// ── Keybinding reference generators ──────────────────────────────────────────

pub(super) fn keybindings_reference_vim() -> String {
    "\
VimCode — Vim Mode Keybinding Reference
========================================
Use / to search.  :Keymaps to add custom overrides.
Commands shown on the right (e.g. :def) can be remapped via :map n <key> :command

── Movement ────────────────────────────────────────────
h j k l             Left / down / up / right
w b e ge            Word forward / back / end / prev-end
W B E gE            WORD forward / back / end / prev-end
0  ^  $  g_         Line start / first non-blank / end / last non-blank
f{c} F{c}           Find char forward / backward
t{c} T{c}           Till char forward / backward
;  ,                Repeat / reverse last f/t/F/T
gg  G               First / last line  ({N}gg = line N)
{  }                Paragraph backward / forward
(  )                Sentence backward / forward
H M L               Screen top / middle / bottom
%                   Matching bracket
+  -                First non-blank of next / prev line
|                   Go to column N
gj gk               Visual (screen) line down / up (wrap mode)
Ctrl+D  Ctrl+U      Half-page down / up
Ctrl+F  Ctrl+B      Page down / up
Ctrl+E  Ctrl+Y      Scroll one line down / up (cursor stays)
Ctrl+O  Ctrl+I      Jump list back / forward

── Editing ─────────────────────────────────────────────
i I                 Insert before cursor / at first non-blank
a A                 Append after cursor / at end of line
o O                 Open line below / above
s S                 Substitute char / line
x X                 Delete char under / before cursor
dd D                Delete line / to end of line
cc C                Change line / to end of line
yy Y                Yank line(s)
p P                 Paste after / before
gp gP               Paste, leave cursor after pasted text
]p [p               Paste with indent adjusted
r{c}                Replace char under cursor
R                   Replace (overtype) mode
u  Ctrl+R           Undo / redo
U                   Undo all changes on current line
.                   Repeat last change
~                   Toggle case under cursor
J  gJ               Join lines (with / without space)
Ctrl+A  Ctrl+X      Increment / decrement number
&  g&               Repeat last :s on line / all lines

── Operators (+ motion or text object) ─────────────────
d{motion}           Delete
c{motion}           Change
y{motion}           Yank
>{motion}  >>       Indent
<{motion}  <<       Dedent
={motion}  ==       Auto-indent
g~{motion}          Toggle case
gu{motion}          Lowercase
gU{motion}          Uppercase
gq{motion}          Reflow to textwidth
gw{motion}          Reflow, keep cursor
g?{motion}          ROT13 encode
!{motion}{cmd}      Filter through shell command
zf{motion}          Create fold

── Text Objects ────────────────────────────────────────
iw aw               Inner / around word
iW aW               Inner / around WORD
is as               Inner / around sentence
ip ap               Inner / around paragraph
i\" a\"               Inner / around double quotes
i' a'               Inner / around single quotes
i` a`               Inner / around backticks
i( a(  ib ab        Inner / around parentheses
i{ a{  iB aB        Inner / around braces
i[ a[               Inner / around brackets
i< a<               Inner / around angle brackets
it at               Inner / around HTML/XML tag

── g-Commands ──────────────────────────────────────────
gg                  Go to first line
gd                  Go to definition (LSP)                :def
gr                  Find references (LSP)                 :refs
gy                  Go to type definition (LSP)           :LspTypedef
gi                  Insert at last insert position
gI                  Insert at column 1
gf gF               Open file / file:line under cursor
gx                  Open URL in default application
gt gT               Next / previous tab
gv                  Reselect last visual selection
gn gN               Select next / prev search match
g* g#               Search word (partial match)
g; g,               Older / newer change position
g.                  Go to last change
ga                  Print ASCII value of char
g8                  Print UTF-8 hex bytes
go                  Go to byte offset N
gcc                 Toggle line comment
gD                  Peek git change (diff popup)      :DiffPeek

── z-Commands ──────────────────────────────────────────
zz zt zb            Scroll cursor to center / top / bottom
z<CR> z. z-         Scroll + move to first non-blank
zh zl               Scroll horizontally left / right
zH zL               Scroll half-screen horizontally
zs ze               Scroll cursor to left / right edge
za zo zc            Toggle / open / close fold
zA zO zC            Toggle / open / close fold recursively
zR zM               Open all / close all folds
zd zD               Delete fold / recursively
zf{motion} zF       Create fold / for N lines
zv                  Open folds to show cursor
zx                  Recompute folds
zj zk               Next / previous fold

── Search & Marks ──────────────────────────────────────
/ ?                 Search forward / backward
n N                 Next / previous match
* #                 Search word under cursor (bounded)
m{a-z}              Set local mark
'{a-z}  `{a-z}     Jump to mark line / position
m{A-Z}              Set global mark
'' ``               Previous position (line / exact)
'. `.               Last edit (line / exact)

── Macros & Registers ──────────────────────────────────
q{a-z}              Record macro    q  Stop recording
@{a-z}              Play macro      @@  Repeat last
@:                  Repeat last ex command
\"{reg}              Use register for next yank/delete/paste

── Window Commands (Ctrl+W prefix) ─────────────────────
Ctrl+W h/j/k/l     Focus left / down / up / right        :wincmd h/j/k/l
Ctrl+W w  W         Cycle next / previous window          :wincmd w
Ctrl+W c  q         Close window                          :close
Ctrl+W o            Close all other windows               :only
Ctrl+W s  v         Horizontal / vertical split           :split  :vsplit
Ctrl+W e  E         Split editor group right / down       :wincmd e/E
Ctrl+W n            New window                            :new
Ctrl+W + - < > =    Resize splits                         :wincmd +/-/</>
Ctrl+W _ |          Maximize height / width               :wincmd _/|
Ctrl+W H/J/K/L     Move to far left/bottom/top/right     :wincmd H/J/K/L
Ctrl+W T            Move window to new editor group       :wincmd T
Ctrl+W x            Exchange with next window             :wincmd x
Ctrl+W r R          Rotate windows forward / backward     :wincmd r/R
Ctrl+W p            Previous window                       :wincmd p
Ctrl+W t b          Top / bottom window                   :wincmd t/b
Ctrl+W f            Split + open file under cursor        :wincmd f
Ctrl+W d            Split + go to definition (LSP)        :wincmd d

── Bracket Navigation ──────────────────────────────────
]c [c               Next / prev git hunk                  :nexthunk / :prevhunk
]d [d               Next / prev LSP diagnostic            :nextdiag / :prevdiag
[[ ]]               Section backward / forward
[] ][               Section end backward / forward
[m ]m               Method start backward / forward
[M ]M               Method end backward / forward
[{ ]}               Unmatched { / }
[( ])               Unmatched ( / )
[* ]*               Comment block start / end
[z ]z               Fold start / end

── Visual Mode ─────────────────────────────────────────
v V Ctrl+V          Char / line / block visual mode
Escape              Exit visual mode
o O                 Swap cursor to other end / corner
d x                 Delete selection
c s                 Change selection
y                   Yank selection
p P                 Paste over selection
> <                 Indent / dedent
= ~                 Auto-indent / toggle case
u U                 Lowercase / uppercase
r{c}                Replace all chars with {c}
I A                 Block insert / append (block mode)
J gJ                Join lines (with / without space)
gq gw               Reflow to textwidth
gc                  Toggle comment
g Ctrl+A/X          Sequential increment / decrement
:                   Command mode with '<,'> range

── Insert Mode ─────────────────────────────────────────
Escape              Return to normal mode
Ctrl+W              Delete word before cursor
Ctrl+U              Delete to start of line
Ctrl+T  Ctrl+D      Indent / dedent current line
Ctrl+R {reg}        Insert register contents
Ctrl+N  Ctrl+P      Next / prev completion
Ctrl+O              Execute one normal-mode command
Ctrl+E  Ctrl+Y      Insert char from line below / above
Ctrl+A              Insert previously-inserted text
Ctrl+V {char}       Insert next char literally
Ctrl+Space          Trigger completion popup

── Leader Key (default: Space) ─────────────────────────
<leader>rn          LSP rename symbol                     :Rename
<leader>gf          LSP format buffer                     :Lformat
<leader>gi          LSP go to implementation              :LspImpl

── Panel / Global Shortcuts ────────────────────────────
Ctrl+P              Fuzzy file finder                     :fuzzy
Ctrl+G              File info (name, line, col, %)
Ctrl+B              Toggle sidebar                        :sidebar
Ctrl+T              Toggle terminal panel                 :terminal
Ctrl+\\              Split editor right
Ctrl+1..9           Focus editor group by position
Alt+E               Focus file explorer
Alt+F               Focus search panel
Alt+D               Add cursor at next match
Ctrl+Shift+L        Select all occurrences
Ctrl+Shift+P        Command palette                       :palette
F1                  Command palette                       :palette
K                   LSP hover info                        :hover
F5/F6/F9/F10/F11   Debug: start/pause/BP/step-over/step-into
                                                          :debug/:pause/:brkpt/:stepover/:stepin
Shift+F5/F11        Debug: stop / step-out                :stop/:stepout
Alt+M               Toggle Vim ↔ VSCode mode
Ctrl+Tab            MRU tab switcher

── Source Control Panel ────────────────────────────
s                   Stage / unstage file (on header: bulk)
d                   Discard changes on file
D                   Discard all unstaged (on CHANGES header)
c                   Open commit input       Enter  Commit
p P f               Push / pull / fetch
r                   Refresh
Tab                 Expand / collapse section
q  Escape           Unfocus panel

── Diff Peek Popup (gD) ───────────────────────────
gD                  Open diff peek on hunk            :DiffPeek
s                   Stage hunk
r                   Revert hunk
q  Escape           Close popup

── Common Ex Commands ──────────────────────────────────
:w :wa :wq :x       Save / all / save+quit
:q :q! :qa :qa!     Quit / force / all / force-all
:e <file>  :e!      Open file / reload
:sp :vs              Horizontal / vertical split
:tabnew :tabclose   New / close tab
:s/pat/rep/[gi]     Substitute on line
:%s/pat/rep/[gi]    Substitute all lines
:g/pat/cmd          Global command
:set <opt>          Query/change setting
:noh                Clear search highlight
:Gdiff :Gblame      Git diff / blame
:Keymaps            Edit custom keymaps
:Keybindings        This reference
:colorscheme <name> Change theme
:ExtInstall <name>  Install extension
:LspInfo            LSP status
:help               Help topics
"
    .to_string()
}

pub(super) fn keybindings_reference_vscode() -> String {
    "\
VimCode — VSCode Mode Keybinding Reference
===========================================
Use Ctrl+F or / to search.
Remap keys: F1 → \"Open Keyboard Shortcuts\", or :map n <key> :command

── Editing ─────────────────────────────────────────────
Ctrl+Z              Undo
Ctrl+Y              Redo
Ctrl+A              Select all
Ctrl+C              Copy selection
Ctrl+X              Cut selection (or cut line if empty)
Ctrl+V              Paste
Ctrl+/              Toggle line comment
Ctrl+Shift+K        Delete current line
Ctrl+Enter          Insert blank line below
Ctrl+Shift+Enter    Insert blank line above
Ctrl+L              Select current line
Ctrl+BackSpace      Delete word backward
Ctrl+Delete         Delete word forward
Tab                 Insert tab / indent
Shift+Tab           Outdent

── Navigation ──────────────────────────────────────────
Arrow keys          Move cursor
Home / End          Smart line start / line end
Ctrl+Right/Left     Move word forward / backward
Ctrl+Home/End       Document start / end
Page Up / Down      Page up / down
Ctrl+G              Go to line (prompt)
Ctrl+P              Fuzzy file finder                     :fuzzy
Ctrl+Shift+P        Command palette                       :palette

── Selection ───────────────────────────────────────────
Shift+Arrow         Extend selection by char / line
Shift+Home/End      Extend to line start / end
Ctrl+Shift+Right    Extend selection word right
Ctrl+Shift+Left     Extend selection word left
Ctrl+Shift+Home     Extend to document start
Ctrl+Shift+End      Extend to document end
Ctrl+D              Select word; repeat = add next occurrence
Ctrl+Shift+L        Select all occurrences (multi-cursor)

── Multi-Cursor ────────────────────────────────────────
Alt+Shift+Up/Down   Add cursor above / below
Ctrl+D              Progressive: word → next occurrence
Ctrl+Shift+L        All occurrences at once
Escape              Collapse to single cursor

── Line Operations ─────────────────────────────────────
Alt+Up / Down       Move line(s) up / down
Alt+Z               Toggle word wrap

── Indentation ─────────────────────────────────────────
Ctrl+]              Indent
Ctrl+[              Outdent
Ctrl+Shift+[        Fold region
Ctrl+Shift+]        Unfold region

── Ctrl+K Chord (press Ctrl+K, then second key) ───────
Ctrl+K, Ctrl+C      Add line comment
Ctrl+K, Ctrl+U      Remove line comment
Ctrl+K, Ctrl+W      Close all editors in group
Ctrl+K, Ctrl+F      Format document

── Panel & View ────────────────────────────────────────
F1                  Command palette                       :palette
F10                 Toggle menu bar
Ctrl+B              Toggle sidebar                        :sidebar
Ctrl+J / Ctrl+`     Toggle terminal panel                 :terminal
Ctrl+,              Open settings
Ctrl+\\              Split editor right
Ctrl+1..9           Focus editor group by position
Alt+E               Focus file explorer
Alt+F               Focus search panel
Ctrl+Tab            MRU tab switcher
Ctrl+Q              Quit

── Debug Keys ──────────────────────────────────────────
F5                  Start / continue debugging            :debug
Shift+F5            Stop debugging                        :stop
F6                  Pause debugger                        :pause
F9                  Toggle breakpoint                     :brkpt
F10                 Step over                             :stepover
F11                 Step into                             :stepin
Shift+F11           Step out                              :stepout
F2                  Rename symbol (LSP)                   :Rename
F12                 Go to definition (LSP)                :def
Shift+F12           Find references (LSP)                 :refs
Ctrl+F12            Go to implementation (LSP)            :LspImpl
Shift+Alt+F         Format document (LSP)                 :Lformat

── Panel Shortcuts (configurable in settings) ──────────
Ctrl+P              Fuzzy file finder                     :fuzzy
Ctrl+G              Go to line / file info
Ctrl+B              Toggle sidebar                        :sidebar
Ctrl+T              Toggle terminal                       :terminal
Alt+E               Focus explorer
Alt+F               Focus search
Alt+D               Add cursor at next match
Ctrl+Shift+L        Select all occurrences
Alt+M               Toggle Vim ↔ VSCode mode

── Source Control Panel ────────────────────────────
s                   Stage / unstage file (on header: bulk)
d                   Discard changes on file
D                   Discard all unstaged (on CHANGES header)
c                   Open commit input       Enter  Commit
p P f               Push / pull / fetch
r                   Refresh
Tab                 Expand / collapse section
q  Escape           Unfocus panel

── Diff Peek Popup (gD) ───────────────────────────
gD                  Open diff peek on hunk            :DiffPeek
s                   Stage hunk
r                   Revert hunk
q  Escape           Close popup

── Common Commands (: prefix) ──────────────────────────
:w :wa :wq           Save / all / save+quit
:q :q!               Quit / force quit
:e <file>            Open file
:sp :vs              Horizontal / vertical split
:tabnew :tabclose    New / close tab
:s/pat/rep/[gi]      Substitute on line
:%s/pat/rep/[gi]     Substitute all lines
:set <opt>           Query/change setting
:Gdiff :Gblame       Git diff / blame
:Keymaps             Edit custom keymaps
:Keybindings         This reference
:colorscheme <name>  Change theme
:ExtInstall <name>   Install extension
:LspInfo             LSP status
:help                Help topics
"
    .to_string()
}

/// Encode a keypress into the canonical string used for keymap matching.
/// Must produce the same format as `parse_key_sequence` output.
fn encode_keypress(key_name: &str, unicode: Option<char>, ctrl: bool) -> String {
    if ctrl {
        // Ctrl combinations: <C-x>
        let base = if let Some(ch) = unicode {
            ch.to_lowercase().to_string()
        } else {
            // Special keys with Ctrl: <C-Space>, <C-Tab>, etc.
            match key_name {
                "space" | "Space" => "Space".to_string(),
                "Tab" => "Tab".to_string(),
                _ => key_name.to_lowercase(),
            }
        };
        format!("<C-{base}>")
    } else if let Some(ch) = unicode {
        ch.to_string()
    } else {
        // Special keys: <Escape>, <Return>, <Space>, etc.
        match key_name {
            "Escape" => "<Escape>".to_string(),
            "Return" => "<Return>".to_string(),
            "BackSpace" => "<BS>".to_string(),
            "space" | "Space" => "<Space>".to_string(),
            "Tab" => "<Tab>".to_string(),
            other => format!("<{other}>"),
        }
    }
}

/// Decode an encoded keypress string back into (key_name, unicode, ctrl).
fn decode_keypress(encoded: &str) -> (String, Option<char>, bool) {
    if encoded.starts_with("<C-") && encoded.ends_with('>') {
        let inner = &encoded[3..encoded.len() - 1];
        match inner {
            "Space" => ("space".to_string(), Some(' '), true),
            "Tab" => ("Tab".to_string(), None, true),
            s => {
                let ch = s.chars().next();
                let key = ch.map(|c| c.to_string()).unwrap_or_default();
                (key.clone(), ch, true)
            }
        }
    } else if encoded.starts_with('<') && encoded.ends_with('>') {
        let inner = &encoded[1..encoded.len() - 1];
        match inner {
            "Escape" => ("Escape".to_string(), None, false),
            "Return" => ("Return".to_string(), None, false),
            "BS" => ("BackSpace".to_string(), None, false),
            "Space" => ("space".to_string(), Some(' '), false),
            "Tab" => ("Tab".to_string(), None, false),
            other => (other.to_string(), None, false),
        }
    } else {
        let ch = encoded.chars().next();
        let key = ch.map(|c| c.to_string()).unwrap_or_default();
        (key, ch, false)
    }
}

/// State for the inline diff peek popup (preview a git diff hunk).
pub struct DiffPeekState {
    /// Index into the buffer's `diff_hunks` array.
    pub hunk_index: usize,
    /// Buffer line the popup is anchored to (0-indexed).
    pub anchor_line: usize,
    /// Raw hunk lines (with +/-/space prefix) for display.
    pub hunk_lines: Vec<String>,
    /// The file_header from the hunk (needed for stage/revert).
    pub file_header: String,
    /// The Hunk itself (needed for stage/revert).
    pub hunk: git::Hunk,
}

pub struct Engine {
    // --- Multi-buffer/window state ---
    pub buffer_manager: BufferManager,
    pub windows: HashMap<WindowId, Window>,
    /// Editor groups stored by ID (recursive split layout).
    pub editor_groups: HashMap<GroupId, EditorGroup>,
    /// ID of the currently focused editor group.
    pub active_group: GroupId,
    /// Previously focused editor group (for CTRL-W p).
    pub prev_active_group: Option<GroupId>,
    /// Recursive binary tree of group splits.
    pub group_layout: GroupLayout,
    /// Counter for generating unique GroupIds.
    next_group_id: usize,
    next_window_id: usize,
    next_tab_id: usize,
    /// Active tab drag-and-drop operation (set by UI on drag start).
    pub tab_drag: Option<TabDragState>,
    /// Current mouse position during a tab drag (for rendering ghost/overlay).
    pub tab_drag_mouse: Option<(f64, f64)>,
    /// Computed drop zone for the current tab drag (updated each frame).
    pub tab_drop_zone: DropZone,

    // --- Preview mode ---
    /// The buffer currently in preview mode (at most one at a time).
    pub preview_buffer_id: Option<BufferId>,

    // --- Global state (not per-window) ---
    pub mode: Mode,
    /// Accumulates typed characters in Command/Search mode.
    pub command_buffer: String,
    /// Cursor position within `command_buffer` (char index, 0 = before first char).
    pub command_cursor: usize,
    /// Wildmenu (command-line Tab completion) state.
    pub wildmenu_items: Vec<String>,
    /// Currently selected wildmenu item index, or `None` for common-prefix state.
    pub wildmenu_selected: Option<usize>,
    /// Original command buffer before wildmenu was opened (for cycling back).
    pub wildmenu_original: String,
    /// Status message shown in the command line area (e.g. "written", errors).
    pub message: String,
    /// Current search query (from last `/` or `?` search).
    pub search_query: String,
    /// Char-offset pairs (start, end) for all search matches in active buffer.
    pub search_matches: Vec<(usize, usize)>,
    /// Index into `search_matches` for the current match.
    pub search_index: Option<usize>,
    /// Direction of the last search operation.
    pub search_direction: SearchDirection,
    /// Cursor position when search mode was entered (for incremental search)
    search_start_cursor: Option<Cursor>,

    // --- Find/Replace state ---
    /// Replacement text for current operation
    #[allow(dead_code)] // Reserved for future UI state tracking
    pub replace_text: String,
    /// Replace flags: 'g' (global), 'c' (confirm), 'i' (case-insensitive)
    #[allow(dead_code)] // Reserved for future UI state tracking
    pub replace_flags: String,

    /// Pending key for multi-key sequences (e.g. 'g' for gg, 'd' for dd).
    pub pending_key: Option<char>,
    /// Parsed user keymaps from settings (rebuilt on settings change).
    pub user_keymaps: Vec<UserKeymap>,
    /// Accumulated keypress buffer for multi-key user keymap matching.
    pub keymap_buf: Vec<String>,
    /// Guard: true while replaying buffered keys through handle_key.
    pub keymap_replaying: bool,
    /// Set by `focus_window_direction` when navigation overflows the window list.
    /// `Some(false)` = tried to go left past first window, `Some(true)` = right past last.
    /// Consumed by the UI backend to move focus to sidebar/toolbar.
    pub window_nav_overflow: Option<bool>,

    // --- Registers (yank/delete storage) ---
    /// Named registers: 'a'-'z' plus '"' (unnamed default). Value is (content, is_linewise).
    pub registers: HashMap<char, (String, bool)>,
    /// Currently selected register for next yank/delete/paste (set by "x prefix).
    pub selected_register: Option<char>,

    // --- Marks ---
    /// Marks per buffer: BufferId -> (mark_char -> Cursor position)
    /// Supports 'a'-'z' for file-local marks
    pub marks: HashMap<BufferId, HashMap<char, Cursor>>,

    // --- Visual mode state ---
    /// Visual mode anchor point (where visual selection started).
    pub visual_anchor: Option<Cursor>,

    // --- Count state ---
    /// Accumulated count for commands (e.g., 5j, 3dd). None means no count entered yet.
    pub count: Option<usize>,

    // --- Character find state ---
    /// Last character find motion: (motion_type, target_char)
    /// motion_type: 'f', 'F', 't', 'T'
    pub last_find: Option<(char, char)>,

    // --- Operator state ---
    /// Pending operator waiting for a motion (e.g., 'd' for dw, 'c' for cw).
    pub pending_operator: Option<char>,
    /// Pending find operator: (operator, find_type) where find_type is f/t/F/T.
    /// Used for dfx/dtx/dFx/dTx — deferred because we need one more keystroke.
    pub pending_find_operator: Option<(char, char)>,

    // --- Text object state ---
    /// Pending text object modifier: 'i' (inner) or 'a' (around)
    pub pending_text_object: Option<char>,

    // --- Repeat state ---
    /// Last change operation for repeat (.)
    last_change: Option<Change>,
    /// Text accumulated during insert mode for repeat
    insert_text_buffer: String,
    /// When true, Replace mode uses virtual column awareness (gR).
    /// Tabs are expanded to spaces before overwriting.
    virtual_replace: bool,
    /// When insert mode was entered via a change operator (cw, ce, cb, etc.),
    /// stores (motion_char, count) so `.` can replay the full change.
    pending_change_motion: Option<(char, usize)>,

    // --- Settings ---
    /// Editor settings (line numbers, etc.)
    pub settings: Settings,

    // --- Session state (history, window geometry, etc.) ---
    /// Session state persisted across restarts
    pub session: SessionState,
    /// Command/search history — saved to a separate history.json file
    pub history: HistoryState,

    /// Current position in command history (None = typing new command)
    pub command_history_index: Option<usize>,

    /// Temporary buffer for current typing when cycling history
    pub command_typing_buffer: String,

    /// Whether Ctrl-R reverse history search is active
    pub history_search_active: bool,

    /// The search string typed during Ctrl-R history search
    pub history_search_query: String,

    /// The index into command history where the current match was found
    pub history_search_index: Option<usize>,

    /// Current position in search history
    pub search_history_index: Option<usize>,

    /// Temporary buffer for search typing
    pub search_typing_buffer: String,

    // --- Macro recording state ---
    /// Which register is recording (None if not recording).
    pub macro_recording: Option<char>,
    /// Accumulated keystrokes during recording.
    pub recording_buffer: Vec<char>,

    // --- Macro playback state ---
    /// Keys to inject for playback.
    pub macro_playback_queue: VecDeque<char>,
    /// Last macro played (for @@).
    pub last_macro_register: Option<char>,
    /// Prevent infinite recursion.
    pub macro_recursion_depth: usize,

    // --- Git integration ---
    /// Current git branch name (None if not in a git repo or git not available).
    pub git_branch: Option<String>,

    // --- Scroll binding ---
    /// Pairs of windows whose scroll_top should stay in sync (e.g. :Gblame).
    /// Each pair is (primary_window_id, secondary_window_id).
    pub scroll_bind_pairs: Vec<(WindowId, WindowId)>,

    // --- Completion state ---
    /// Current completion candidates (populated on first Ctrl-N/P or auto-trigger).
    pub completion_candidates: Vec<String>,
    /// Index of the currently selected candidate, or None when inactive.
    pub completion_idx: Option<usize>,
    /// Buffer column where the prefix that triggered completion starts.
    pub completion_start_col: usize,
    /// True when the popup was triggered automatically (typing/Ctrl-Space):
    /// Tab accepts the highlighted item. False for Ctrl-N/P (inserts immediately as before).
    pub completion_display_only: bool,

    // --- Project search state ---
    /// Current text typed in the project search input box.
    pub project_search_query: String,
    /// Results from the last `run_project_search` call.
    pub project_search_results: Vec<ProjectMatch>,
    /// Index of the currently highlighted result (0-based).
    pub project_search_selected: usize,
    /// Search mode toggles (case-sensitive, whole word, regex).
    pub project_search_options: SearchOptions,
    /// Receiver for async search results (set while a search thread is running).
    pub project_search_receiver:
        Option<std::sync::mpsc::Receiver<Result<Vec<ProjectMatch>, SearchError>>>,
    /// True while a background search thread is running.
    pub project_search_running: bool,

    // --- Project replace state ---
    /// Current text typed in the project replace input box.
    pub project_replace_text: String,
    /// Receiver for async replace results (set while a replace thread is running).
    pub project_replace_receiver:
        Option<std::sync::mpsc::Receiver<Result<ReplaceResult, SearchError>>>,
    /// True while a background replace thread is running.
    pub project_replace_running: bool,

    // --- LSP state ---
    /// Multi-server LSP coordinator. None until first LSP-capable file is opened.
    pub lsp_manager: Option<LspManager>,
    /// Per-file diagnostics from LSP servers.
    pub lsp_diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    /// Hover text to display (set on K keypress, cleared on any movement).
    pub lsp_hover_text: Option<String>,
    /// Whether LSP completion is currently active (vs buffer-word completion).
    #[allow(dead_code)]
    pub lsp_completion_active: bool,
    /// Request ID of the pending completion request.
    pub lsp_pending_completion: Option<i64>,
    /// Request ID of the pending hover request.
    pub lsp_pending_hover: Option<i64>,
    /// Request ID of the pending definition request.
    pub lsp_pending_definition: Option<i64>,
    /// Request ID of the pending references request.
    pub lsp_pending_references: Option<i64>,
    /// Request ID of the pending implementation request.
    pub lsp_pending_implementation: Option<i64>,
    /// Request ID of the pending type-definition request.
    pub lsp_pending_type_definition: Option<i64>,
    /// Request ID of the pending signature-help request.
    pub lsp_pending_signature: Option<i64>,
    /// Request ID of the pending formatting request.
    pub lsp_pending_formatting: Option<i64>,
    /// Buffer that triggered a format-on-save; after formatting completes we save it.
    format_on_save_pending: Option<BufferId>,
    /// If true, quit the editor after the deferred format-on-save completes.
    quit_after_format_save: bool,
    /// Set to true when a format-on-save + quit has completed; backends should exit.
    pub format_save_quit_ready: bool,
    /// Request ID of the pending rename request.
    pub lsp_pending_rename: Option<i64>,
    /// Pending semantic tokens requests: maps request_id → file path.
    /// Multiple requests can be in flight simultaneously (e.g. after LSP Initialized).
    pub lsp_pending_semantic_tokens: HashMap<i64, PathBuf>,
    /// Currently visible signature help data (set in insert mode after `(` or `,`).
    pub lsp_signature_help: Option<SignatureHelpData>,
    /// Tracks whether we need to send didChange on next poll (debounce).
    lsp_dirty_buffers: HashMap<BufferId, bool>,
    /// Request ID of the pending code action request.
    pub lsp_pending_code_action: Option<i64>,
    /// Pending document symbol request ID.
    pub lsp_pending_document_symbols: Option<i64>,
    /// Pending workspace symbol request ID.
    pub lsp_pending_workspace_symbols: Option<i64>,
    /// The (path, line) for which the pending code action request was made.
    lsp_code_action_request_ctx: Option<(PathBuf, usize)>,
    /// Cached code actions per file path and line number.
    pub lsp_code_actions: HashMap<PathBuf, HashMap<usize, Vec<lsp::CodeAction>>>,
    /// Last (path, line) for which code actions were requested — avoids re-requesting same line.
    lsp_code_action_last_line: Option<(PathBuf, usize)>,
    /// When true, the next CodeActionResponse should display a popup (on-demand request).
    lsp_show_code_action_popup_pending: bool,
    /// Actions shown in the current code-action dialog, indexed by dialog button position.
    pending_code_action_choices: Vec<lsp::CodeAction>,

    /// Set when cursor moves; backends flush the actual hook after a debounce delay (150ms).
    pub cursor_move_pending: Option<std::time::Instant>,

    /// Language IDs for which a background install is in progress.
    lsp_installing: std::collections::HashSet<String>,
    /// Language IDs for which a Mason registry lookup is already in flight.
    lsp_lookup_in_flight: std::collections::HashSet<String>,

    // --- Leader key ---
    /// Accumulated keys after leader was pressed; `None` when not in leader mode.
    leader_partial: Option<String>,

    // --- Jump list ---
    /// List of (file_path, line, col) jump positions. Max 100 entries.
    jump_list: Vec<(Option<PathBuf>, usize, usize)>,
    /// Current position in jump list (points past the last entry when at newest).
    jump_list_pos: usize,

    // --- Search word under cursor ---
    /// Whether current search uses word boundaries (set by * and #).
    search_word_bounded: bool,

    // --- Workspace ---
    /// Path to the loaded `.vimcode-workspace` file, if any.
    pub workspace_file: Option<PathBuf>,
    /// Resolved workspace root directory (derived from `workspace_file`), if any.
    pub workspace_root: Option<PathBuf>,
    /// Snapshot of user settings before any workspace/folder overlay was applied.
    /// Set when entering a workspace, restored when leaving. `None` if no overlay active.
    pub base_settings: Option<Box<Settings>>,

    // --- Sidebar focus (shared by all backends) ---
    /// Whether the Explorer sidebar panel has keyboard focus.
    pub explorer_has_focus: bool,
    /// Whether the Search sidebar panel has keyboard focus.
    pub search_has_focus: bool,

    // --- Source Control panel ---
    /// Cached file statuses from the last `sc_refresh()` call.
    pub sc_file_statuses: Vec<git::FileStatus>,
    /// Cached worktree list from the last `sc_refresh()` call.
    pub sc_worktrees: Vec<git::WorktreeEntry>,
    /// Flat selection index across all SC sections (staged/unstaged/worktrees).
    pub sc_selected: usize,
    /// Which sections are expanded: [staged, unstaged, worktrees, log].
    pub sc_sections_expanded: [bool; 4],
    /// Whether the Source Control panel currently has keyboard focus.
    pub sc_has_focus: bool,
    /// Ahead/behind counts relative to upstream (cached alongside `sc_refresh`).
    pub sc_ahead: u32,
    pub sc_behind: u32,
    /// Cached git log entries (recent commits) from the last `sc_refresh()` call.
    pub sc_log: Vec<git::GitLogEntry>,
    /// Commit message being typed in the SC panel input row.
    pub sc_commit_message: String,
    /// Byte-offset cursor position within `sc_commit_message`.
    pub sc_commit_cursor: usize,
    /// True when the SC commit input row has keyboard focus.
    pub sc_commit_input_active: bool,
    /// Which action button (0=Commit 1=Push 2=Pull 3=Sync) is keyboard-focused, or None.
    pub sc_button_focused: Option<usize>,
    /// Which action button the mouse is hovering over, or None.
    pub sc_button_hovered: Option<usize>,

    // Branch picker popup (opened with `b` in SC panel)
    pub sc_branch_picker_open: bool,
    pub sc_branch_picker_query: String,
    pub sc_branch_picker_branches: Vec<git::BranchEntry>,
    pub sc_branch_picker_selected: usize,
    /// Create-branch mode (opened with `B` in SC panel)
    pub sc_branch_create_mode: bool,
    pub sc_branch_create_input: String,
    /// SC panel help dialog visible
    pub sc_help_open: bool,

    // --- Plugin system ---
    /// Manages loaded Lua plugins. `None` if no plugins dir or plugins disabled.
    pub plugin_manager: Option<plugin::PluginManager>,

    // --- Comment toggling ---
    /// Runtime overrides for comment styles, keyed by LSP language ID.
    /// Populated from extension manifests and `vimcode.set_comment_style()` Lua API.
    pub comment_overrides: HashMap<String, comment::CommentStyleOwned>,
    /// Highlight query overrides from extension manifests, keyed by LSP language ID.
    pub highlight_overrides: HashMap<String, String>,

    // --- Fuzzy file finder ---
    /// Project root directory for the fuzzy finder.
    pub cwd: PathBuf,
    /// Whether the fuzzy finder modal is open.
    // --- Tab switcher (Alt+Tab MRU popup) ---
    /// Whether the tab switcher popup is open.
    pub tab_switcher_open: bool,
    /// Index of the currently highlighted item in the MRU list.
    pub tab_switcher_selected: usize,
    /// MRU-ordered list of (group_id, tab_index) pairs.
    /// Most recently used is at index 0.
    pub tab_mru: Vec<(GroupId, usize)>,

    /// Back/forward tab navigation history.
    /// Each entry is (GroupId, TabId) at the time of the switch.
    pub tab_nav_history: Vec<(GroupId, TabId)>,
    /// Current position in `tab_nav_history` (index of the entry we're viewing).
    pub tab_nav_index: usize,
    /// True when navigating via back/forward (suppresses pushing to history).
    tab_nav_navigating: bool,

    // --- Quickfix state ---
    /// Quickfix list populated by :grep / :vimgrep.
    pub quickfix_items: Vec<ProjectMatch>,
    /// Currently selected quickfix item (0-based).
    pub quickfix_selected: usize,
    /// Whether the quickfix panel is visible.
    pub quickfix_open: bool,
    /// Whether the quickfix panel has keyboard focus.
    pub quickfix_has_focus: bool,
    /// Whether the debug sidebar has keyboard focus.
    pub dap_sidebar_has_focus: bool,

    // --- Unified picker modal ---
    /// Whether the unified picker modal is open.
    pub picker_open: bool,
    /// Which data source is backing the picker.
    pub picker_source: PickerSource,
    /// Current query typed in the picker input.
    pub picker_query: String,
    /// Full source items (pre-loaded sources only; empty for live sources like Grep).
    pub picker_all_items: Vec<PickerItem>,
    /// Filtered/scored items currently displayed (capped).
    pub picker_items: Vec<PickerItem>,
    /// Index of the currently highlighted item.
    pub picker_selected: usize,
    /// Scroll offset for the result list.
    pub picker_scroll_top: usize,
    /// Title shown in the picker header.
    pub picker_title: String,
    /// Preview pane content for the selected item, or None for no-preview sources.
    pub picker_preview: Option<PickerPreview>,

    // --- Breadcrumb focus mode ---
    /// Whether breadcrumb keyboard navigation is active (entered via `<leader>b`).
    pub breadcrumb_focus: bool,
    /// Index of the currently highlighted breadcrumb segment (0-based).
    pub breadcrumb_selected: usize,
    /// Cached breadcrumb segments for the active group, rebuilt on focus entry.
    pub breadcrumb_segments: Vec<BreadcrumbSegmentInfo>,
    /// When `Some`, `picker_populate_document_symbols` filters to symbols
    /// whose container matches this value. `Some(None)` = top-level only.
    pub breadcrumb_scoped_parent: Option<Option<String>>,

    // --- Two-way diff state ---
    /// The pair of windows currently in diff mode, or None when diff is off.
    pub diff_window_pair: Option<(WindowId, WindowId)>,
    /// Per-window per-line diff status.  Keyed by WindowId, value is a Vec
    /// with one entry per buffer line.
    pub diff_results: HashMap<WindowId, Vec<DiffLine>>,
    /// Aligned diff sequences for visual padding.  Each entry maps a visual
    /// row to a real buffer line or a padding filler so that matching content
    /// appears at the same row on both sides of a diff split.
    pub diff_aligned: HashMap<WindowId, Vec<AlignedDiffEntry>>,
    /// Whether unchanged sections in the diff view are hidden (folded).
    pub diff_unchanged_hidden: bool,

    /// Receiver for background `git show HEAD:file` results used by
    /// the Source Control panel's click-to-diff flow.  The thread sends
    /// `(abs_path, head_content)`.
    pub sc_diff_rx: Option<std::sync::mpsc::Receiver<(PathBuf, String)>>,
    /// Window ID of the tab pre-opened for the pending SC diff.
    pub sc_diff_pending_win: Option<WindowId>,

    // --- Inline diff peek popup ---
    /// Git diff peek popup state, or `None` when no peek is active.
    pub diff_peek: Option<DiffPeekState>,

    // --- Clipboard callbacks (set by UI backend) ---
    /// Read text from the system clipboard.  Set by the GTK/TUI backend at startup.
    /// Returns Ok(text) or Err(error_message).
    pub clipboard_read: Option<Box<dyn Fn() -> Result<String, String>>>,
    /// Write text to the system clipboard.  Set by the GTK/TUI backend at startup.
    /// Returns Err(error_message) on failure.
    #[allow(clippy::type_complexity)]
    pub clipboard_write: Option<Box<dyn Fn(&str) -> Result<(), String>>>,
    /// Whether a mouse drag selection is currently active.
    pub mouse_drag_active: bool,
    /// Window where the current drag selection originated.  Drag events in
    /// other windows are ignored until mouse-up so selections don't leak
    /// across editor groups.
    pub mouse_drag_origin_window: Option<WindowId>,
    /// When true, drag extends selection word-wise (set by double-click).
    pub mouse_drag_word_mode: bool,
    /// Original word boundaries from double-click (start_col, end_col, line).
    pub mouse_drag_word_origin: Option<(usize, usize, usize)>,

    // --- Menu bar / debug toolbar ---
    /// Whether the VSCode-style menu bar strip is visible.
    pub menu_bar_visible: bool,
    /// Index of the currently open top-level menu dropdown (None = bar visible but no dropdown).
    pub menu_open_idx: Option<usize>,
    /// Whether the debug toolbar strip is shown (persistent for now; later: only during DAP session).
    pub debug_toolbar_visible: bool,
    /// True while a DAP debug session is active.
    pub dap_session_active: bool,
    /// Index of the keyboard-highlighted item in the currently open menu dropdown.
    pub menu_highlighted_item: Option<usize>,

    // --- DAP (Debug Adapter Protocol) state ---
    /// Multi-adapter DAP coordinator. None until first debug session is started.
    pub dap_manager: Option<DapManager>,
    /// Thread ID of the currently stopped thread (set on Stopped event).
    pub dap_stopped_thread: Option<u64>,
    /// Per-file breakpoints: absolute file path → sorted list of 1-based line numbers.
    /// Per-file breakpoints: absolute file path → sorted list of breakpoint info.
    pub dap_breakpoints: HashMap<String, Vec<BreakpointInfo>>,
    /// Sequence number of the initialize request (used to detect its response).
    /// Needed because codelldb omits the `command` field from responses.
    pub dap_seq_initialize: Option<u64>,
    /// Sequence number of the launch request (used to detect session start).
    pub dap_seq_launch: Option<u64>,
    /// Current stopped location: (absolute_file_path, 1-based line number).
    /// Set when the adapter reports a Stopped event and stack trace is resolved.
    /// Cleared on Continued or Exited.
    pub dap_current_line: Option<(String, u64)>,
    /// Call-stack frames captured on the last Stopped event.
    /// Cleared on Continued or Exited.
    pub dap_stack_frames: Vec<StackFrame>,
    /// Variables in scope for the current stopped frame (populated after scopes response).
    pub dap_variables: Vec<DapVariable>,
    /// DAP output console: stdout/stderr from the debugged process + adapter messages.
    /// Capped at 1000 lines; new lines appended, oldest dropped from the front.
    pub dap_output_lines: Vec<String>,
    /// Carry buffer for incomplete ANSI escape sequences split across consecutive
    /// DAP output events.  The tail of one event may be `\x1b[38;2;` while the
    /// next event starts with `97;175;239m` — we prepend the carry so the combined
    /// string is stripped correctly.
    pub dap_ansi_carry: String,
    /// Index of the currently selected stack frame in the panel.
    pub dap_active_frame: usize,
    /// Set of variable-reference IDs that the user has expanded (to show child variables).
    pub dap_expanded_vars: HashSet<u64>,
    /// Child variables fetched for expanded entries; keyed by the parent variablesReference.
    pub dap_child_variables: HashMap<u64, Vec<DapVariable>>,
    /// Most-recent expression evaluation result (`:DapEval`), or `None`.
    pub dap_eval_result: Option<String>,
    /// Tracks what the last `variables` request was for:
    /// `0` = top-level scope (store result in `dap_variables`),
    /// non-zero = child expansion (store in `dap_child_variables[key]`).
    pub dap_pending_vars_ref: u64,
    /// Name of the primary scope (e.g. "Locals"). Empty when no scopes received yet.
    pub dap_primary_scope_name: String,
    /// variablesReference for the primary scope. 0 when no scopes received yet.
    /// Used to render the scope header and to skip re-fetch on toggle.
    pub dap_primary_scope_ref: u64,
    /// Additional scope groups beyond the primary "Locals" scope.
    /// Each entry is (scope_name, variablesReference). Shown as expandable
    /// groups at the bottom of the Variables section.
    pub dap_scope_groups: Vec<(String, u64)>,
    /// Which section of the debug sidebar is currently selected.
    pub dap_sidebar_section: DebugSidebarSection,
    /// Selected item index within the active sidebar section.
    pub dap_sidebar_selected: usize,
    /// Per-section scroll offset (top visible item index) for [Variables, Watch, CallStack, Breakpoints].
    pub dap_sidebar_scroll: [usize; 4],
    /// Per-section allocated heights in content rows (excluding header).
    /// Computed by backends and stored for ensure_visible calculations.
    pub dap_sidebar_section_heights: [u16; 4],
    /// Watch expressions added by the user (`:DapWatch <expr>`).
    pub dap_watch_expressions: Vec<String>,
    /// Evaluated values for each watch expression (parallel vec; `None` = not yet evaluated).
    pub dap_watch_values: Vec<Option<String>>,
    /// Debug configurations parsed from `.vscode/launch.json`, or generated.
    pub dap_launch_configs: Vec<LaunchConfig>,
    /// Index into `dap_launch_configs` that will be used on the next F5.
    pub dap_selected_launch_config: usize,
    /// Which panel is shown in the shared bottom area (Terminal or Debug Output).
    pub bottom_panel_kind: BottomPanelKind,
    /// Launch arguments stored between `initialize` send and response receipt.
    /// We defer `launch`/`attach` until the adapter confirms `initialize` to avoid a race
    /// where codelldb processes both requests concurrently and reads arguments
    /// from an uninitialised state (causing program="" / "(empty)" errors).
    /// Tuple: (request_type, args) where request_type is "launch" or "attach".
    pub dap_pending_launch: Option<(String, serde_json::Value)>,
    /// True while the bottom panel should be visible regardless of terminal state.
    /// Set when a debug session starts; keeps the Debug Output tab accessible
    /// even if the user never opened a terminal.
    pub bottom_panel_open: bool,
    /// One-shot flag: backends consume this to auto-switch to the Debug sidebar.
    pub dap_wants_sidebar: bool,
    /// Maps DAP evaluate request seq → watch expression index for watch eval tracking.
    pub dap_pending_watch_seqs: HashMap<u64, usize>,
    /// True after the preLaunchTask has completed successfully; prevents re-running on retry.
    pub dap_pre_launch_done: bool,
    /// Language to resume `dap_start_debug()` with after preLaunchTask completes.
    pub dap_deferred_lang: Option<String>,

    // --- Integrated terminal ---
    /// All open terminal panes (PTY + VT100 parser). Empty until first open.
    pub terminal_panes: Vec<TerminalPane>,
    /// Pending install context for the next `terminal_run_command()` call.
    pub pending_install_context: Option<InstallContext>,
    /// Command that should be run in a visible terminal pane (set by ext_install).
    /// Consumed by the UI layer on the next event loop iteration.
    pub pending_terminal_command: Option<String>,
    /// Index of the currently active terminal pane.
    pub terminal_active: usize,
    /// Whether the terminal panel is visible.
    pub terminal_open: bool,
    /// Whether the terminal panel has keyboard focus.
    pub terminal_has_focus: bool,
    /// Whether the inline find bar is active in the terminal panel.
    pub terminal_find_active: bool,
    /// Current search query in the terminal find bar.
    pub terminal_find_query: String,
    /// Index of the currently highlighted match (wraps via modulo in render).
    pub terminal_find_selected: usize,
    /// (required_scroll_offset, row, col) of each match across all accessible history.
    /// Sorted oldest-to-newest (highest offset first, then top-to-bottom).
    pub terminal_find_matches: Vec<(usize, u16, u16)>,
    /// Whether the terminal panel is in horizontal split view (two panes side-by-side).
    /// When true, pane[0] is left and pane[1] is right; `terminal_active` is 0 or 1.
    pub terminal_split: bool,
    /// Visual column width of the left pane during a split-divider drag resize.
    /// Zero means "use the pane's actual PTY column count".
    /// Set by `terminal_split_set_drag_cols`; cleared by `terminal_split_finalize_drag`.
    pub terminal_split_left_cols: u16,

    // --- Special marks (for '', '., '<, '>) ---
    /// Position before last jump (for '' and `` marks).
    pub last_jump_pos: Option<(usize, usize)>,
    /// Position of last buffer edit (for '. and `. marks).
    pub last_edit_pos: Option<(usize, usize)>,
    /// Position where cursor was when last leaving Insert mode (for `gi`).
    pub last_insert_pos: Option<(usize, usize)>,
    /// Start of last visual selection (for '< and `< marks).
    pub visual_mark_start: Option<(usize, usize)>,
    /// End of last visual selection (for '> and `> marks).
    pub visual_mark_end: Option<(usize, usize)>,
    /// Global marks A-Z: char -> (optional file path, line, col).
    pub global_marks: HashMap<char, (Option<PathBuf>, usize, usize)>,

    // --- gv state (reselect last visual) ---
    /// Visual anchor saved when leaving visual mode (for gv).
    pub last_visual_anchor: Option<Cursor>,
    /// Cursor position saved when leaving visual mode (for gv).
    pub last_visual_cursor: Option<Cursor>,
    /// Visual mode saved when leaving visual mode (for gv).
    pub last_visual_mode: Mode,

    // --- Change list (g; / g,) ---
    /// List of (line, col) positions where buffer changes occurred. Max 100.
    pub change_list: Vec<(usize, usize)>,
    /// Index into change_list for g;/g, navigation (points past end when at newest).
    pub change_list_pos: usize,

    // --- Last inserted text (". register) ---
    /// Text typed during the last Insert mode session (for ". register).
    pub last_inserted_text: String,

    // --- Last ex command (@:) ---
    /// Last executed ex command (for @: repeat).
    pub last_ex_command: Option<String>,

    // --- Last substitute (&) ---
    /// Last substitute (pattern, replacement, flags) for & repeat.
    pub last_substitute: Option<(String, String, String)>,

    // --- Yank highlight (transient visual feedback) ---
    /// Region to highlight briefly after a yank operation: (start, end, is_linewise).
    /// Cleared by the UI backend after ~200 ms.
    pub yank_highlight: Option<(Cursor, Cursor, bool)>,

    /// Matching bracket position (line, col) when cursor is on a bracket char.
    /// Updated at the end of `handle_key()`.
    pub bracket_match: Option<(usize, usize)>,

    // --- Insert mode Ctrl+r pending ---
    /// When true, the next keypress in Insert (or Replace) mode inserts a register's content.
    pub insert_ctrl_r_pending: bool,
    pub insert_ctrl_g_pending: bool,
    /// When true, after one Normal-mode command, auto-return to Insert mode (Ctrl-O).
    pub insert_ctrl_o_active: bool,
    /// When true, next keypress in Insert mode is inserted literally (Ctrl-V).
    pub insert_ctrl_v_pending: bool,
    /// Stores visual block insert/append info: (start_line, end_line, col, is_append).
    /// On Escape from Insert, apply insert_text_buffer to all block lines.
    pub visual_block_insert_info: Option<(usize, usize, usize, bool)>,
    /// Force motion mode: 'v' = charwise, 'V' = linewise.
    /// Set by pressing v/V/CTRL-V while an operator is pending (e.g., dVj).
    pub force_motion_mode: Option<char>,

    // --- Extension system ---
    /// Which extensions are installed / dismissed (persisted to extensions.json).
    pub extension_state: ExtensionState,
    /// Extensions for which an install prompt was shown this session (avoids re-prompting).
    pub prompted_extensions: HashSet<String>,
    /// Name of the extension currently being hinted in the status bar (enables N-to-dismiss).
    pub ext_hint_pending_name: Option<String>,

    // --- Extension registry (remote) ---
    /// Fetched remote registry entries (None until first :ExtRefresh or sidebar open).
    pub ext_registry: Option<Vec<extensions::ExtensionManifest>>,
    /// True while the background registry fetch thread is running.
    pub ext_registry_fetching: bool,
    /// Channel for receiving the registry fetch result from the background thread.
    pub ext_registry_rx:
        Option<std::sync::mpsc::Receiver<Option<Vec<extensions::ExtensionManifest>>>>,

    // --- Extensions sidebar state ---
    /// Whether the Extensions sidebar panel has keyboard focus.
    pub ext_sidebar_has_focus: bool,
    /// Flat selection index across installed + available items.
    pub ext_sidebar_selected: usize,
    /// Filter query typed in the sidebar search box.
    pub ext_sidebar_query: String,
    /// Which sections are expanded: [installed, available].
    pub ext_sidebar_sections_expanded: [bool; 2],
    /// Whether the sidebar search input field is active.
    pub ext_sidebar_input_active: bool,
    /// Extension name pending removal (set when the remove-confirmation dialog opens).
    pub pending_ext_remove: Option<String>,

    /// Pending git remote operation awaiting SSH passphrase from dialog.
    /// Holds `"push"`, `"pull"`, or `"fetch"`.
    pub pending_git_remote_op: Option<String>,

    // --- Settings sidebar panel state ---
    /// Whether the Settings sidebar panel has keyboard focus.
    pub settings_has_focus: bool,
    /// Flat selection index into visible settings rows.
    pub settings_selected: usize,
    /// Scroll offset for the settings panel content.
    pub settings_scroll_top: usize,
    /// Search/filter query typed in the settings panel.
    pub settings_query: String,
    /// Whether the search input is active (user typing a filter).
    pub settings_input_active: bool,
    /// Index into SETTING_DEFS being edited inline (for string/int fields).
    pub settings_editing: Option<usize>,
    /// Buffer for inline string/int editing.
    pub settings_edit_buf: String,
    /// Per-category collapsed state.
    pub settings_collapsed: Vec<bool>,
    /// Per-extension settings: ext_name → { key → value }.
    pub ext_settings: HashMap<String, HashMap<String, String>>,
    /// Per-extension-category collapsed state in the Settings sidebar.
    pub ext_settings_collapsed: HashMap<String, bool>,
    /// When editing an extension setting inline: `(ext_name, key)`.
    pub ext_settings_editing: Option<(String, String)>,

    // --- Virtual text / line annotations ---
    /// Inline annotation text indexed by 0-based buffer line number.
    /// Cleared when the active buffer changes.
    pub line_annotations: HashMap<usize, String>,
    /// Whether current `line_annotations` are blame-sourced (enables rich hover).
    pub blame_annotations_active: bool,
    /// Receiver for async blame results (background thread).
    blame_rx: Option<std::sync::mpsc::Receiver<Vec<crate::core::git::BlameInfo>>>,

    // --- Async shell tasks (plugin background commands) ---
    /// Background shell tasks spawned by plugins via `vimcode.async_shell()`.
    /// Keyed by callback_event name (last-writer-wins: new request replaces old).
    async_shell_tasks: HashMap<String, std::sync::mpsc::Receiver<(bool, String)>>,

    // --- AI assistant panel ---
    /// Conversation history shown in the AI sidebar.
    pub ai_messages: Vec<AiMessage>,
    /// Current input text being composed.
    pub ai_input: String,
    /// Cursor position in `ai_input` (char index, 0 = before first char).
    pub ai_input_cursor: usize,
    /// Whether the AI sidebar has keyboard focus.
    pub ai_has_focus: bool,
    /// Whether the input box is in active editing mode.
    pub ai_input_active: bool,
    /// True while a request is in-flight.
    pub ai_streaming: bool,
    /// Channel for receiving the AI response from the background thread.
    pub ai_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    /// Scroll offset for the conversation history (in lines).
    pub ai_scroll_top: usize,

    // --- AI inline completions (ghost text) ---
    /// Ghost text currently shown at the cursor (first/current alternative).
    /// `None` means no ghost text is visible.
    pub ai_ghost_text: Option<String>,
    /// All completion alternatives returned from the last request.
    pub ai_ghost_alternatives: Vec<String>,
    /// Index into `ai_ghost_alternatives` for the currently shown alternative.
    pub ai_ghost_alt_idx: usize,
    /// Countdown ticker for debouncing inline completions.
    /// Set to ~30 on every insert-mode keystroke; the backend decrements it
    /// each frame and calls `tick_ai_completion()` when it reaches zero.
    pub ai_completion_ticks: Option<u32>,
    /// Channel for receiving ghost text from the background completion thread.
    pub ai_completion_rx: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    /// Tail of the prefix (text before cursor) captured when the last completion
    /// request was fired.  Used on arrival to strip any prefix characters the AI
    /// repeated from the context (e.g. the `"` in `"PlayerObject":` when the
    /// buffer already ends with `"`).
    pub ai_completion_prefix_tail: String,

    /// Maps preview buffer ID → source buffer ID for live markdown preview.
    pub md_preview_links: HashMap<BufferId, BufferId>,

    // --- Swap file crash recovery ---
    /// Buffers whose content changed since the last swap file write.
    swap_write_needed: HashSet<BufferId>,
    /// When we last wrote swap files to disk.
    swap_last_write: std::time::Instant,
    /// Pending swap recovery data (used by the dialog system).
    pub pending_swap_recovery: Option<SwapRecovery>,
    /// Modal dialog displayed over the editor.
    pub dialog: Option<Dialog>,

    /// Spell checker (lazily initialized when `settings.spell` is true).
    pub spell_checker: Option<super::spell::SpellChecker>,
    /// Spell suggestions pending selection (word, suggestions list, input buffer).
    pub spell_suggestions: Option<(String, Vec<String>, String)>,

    // --- VSCode mode state ---
    /// Ctrl+K chord pending: waiting for the second key of a Ctrl+K combo.
    pub vscode_pending_ctrl_k: bool,
    /// True while a VSCode-mode undo group is held open across consecutive
    /// character insertions.  Broken by any non-character action (cursor move,
    /// Ctrl+* command, Backspace, Return, etc.) so that contiguous typing
    /// bursts coalesce into a single undo entry.
    pub vscode_undo_group_open: bool,
    /// Cursor position after the last VSCode-mode character insertion.
    /// Used to detect external cursor moves (mouse click, etc.) that should
    /// break the undo group even though the next key is a character.
    pub vscode_undo_cursor: (usize, usize),

    // --- Extension panels ---
    /// Registered extension panels (name → registration).
    pub ext_panels: HashMap<String, plugin::PanelRegistration>,
    /// Extension panel items: (panel_name, section_name) → items.
    pub ext_panel_items: HashMap<(String, String), Vec<plugin::ExtPanelItem>>,
    /// Which extension panel is currently showing (if any).
    pub ext_panel_active: Option<String>,
    /// Whether the extension panel has keyboard focus.
    pub ext_panel_has_focus: bool,
    /// Flat selection index across all sections.
    pub ext_panel_selected: usize,
    /// Scroll offset for the extension panel.
    pub ext_panel_scroll_top: usize,
    /// Per-panel section expanded state.
    pub ext_panel_sections_expanded: HashMap<String, Vec<bool>>,
    /// Per-panel tree item expand state: (panel_name, item_id) → expanded.
    /// Persists across `set_items` refreshes so plugins don't lose user-toggled state.
    pub ext_panel_tree_expanded: HashMap<(String, String), bool>,
    /// Per-panel input field text: panel_name → current text.
    pub ext_panel_input_text: HashMap<String, String>,
    /// Whether the input field in the active extension panel currently has keyboard focus.
    pub ext_panel_input_active: bool,
    /// Signals backends to switch the sidebar to this extension panel on the next frame.
    /// Set by `panel.reveal()` — backends check + clear each frame.
    pub ext_panel_focus_pending: Option<String>,
    /// Extension panel help popup state
    pub ext_panel_help_open: bool,
    /// Extension panel help bindings: panel_name -> [(key, description)]
    pub ext_panel_help_bindings: HashMap<String, Vec<(String, String)>>,

    // --- Editor hover popup ---
    /// Active editor hover popup with rendered markdown content.
    pub editor_hover: Option<EditorHoverPopup>,
    /// Mouse dwell tracking for editor hover: (buffer_line, buffer_col, timestamp).
    pub editor_hover_dwell: Option<(usize, usize, std::time::Instant)>,
    /// Delayed dismiss deadline — popup lingers until this instant passes.
    pub editor_hover_dismiss_at: Option<std::time::Instant>,
    /// Position where LSP returned null hover — suppresses re-request until mouse moves off.
    lsp_hover_null_pos: Option<(usize, usize)>,
    /// Position of the in-flight LSP hover request (for recording null-response positions).
    lsp_hover_request_pos: Option<(usize, usize)>,
    /// Whether the editor hover popup currently has keyboard focus.
    pub editor_hover_has_focus: bool,
    /// Plugin-provided static hover content per line: line (0-indexed) → markdown.
    pub editor_hover_content: HashMap<usize, String>,
    /// Tab hover tooltip: shortened file path shown when hovering a tab.
    pub tab_hover_tooltip: Option<String>,
    /// Performance profiling log for the last slow keystroke (> 5ms).
    pub perf_log: Option<String>,

    // --- Panel hover popup ---
    /// Active sidebar hover popup with rendered markdown content.
    pub panel_hover: Option<PanelHoverPopup>,
    /// Hover dwell tracking: (panel_name, item_index, timestamp).
    pub panel_hover_dwell: Option<(String, usize, std::time::Instant)>,
    /// Delayed dismiss deadline — popup lingers until this instant passes.
    pub panel_hover_dismiss_at: Option<std::time::Instant>,
    /// Plugin-provided hover content: (panel_name, item_id) -> markdown string.
    pub panel_hover_registry: HashMap<(String, String), String>,

    // --- Context menu ---
    /// Active right-click context menu state (TUI-driven). None when no menu is open.
    pub context_menu: Option<ContextMenuState>,
    /// File selected as "left side" for a two-way diff comparison (via context menu).
    pub diff_selected_file: Option<PathBuf>,
    /// Pending move awaiting user confirmation: (source_path, dest_dir).
    pub pending_move: Option<(PathBuf, PathBuf)>,
    /// Pending delete awaiting user confirmation.
    pub pending_delete: Option<PathBuf>,
    /// Set to true when a file move completes; backends should refresh the explorer tree
    /// and clear this flag.
    pub explorer_needs_refresh: bool,

    /// Inline rename state for the explorer sidebar.  When `Some`, the
    /// sidebar row matching `path` should render an editable text input
    /// instead of the plain filename.
    pub explorer_rename: Option<ExplorerRenameState>,

    /// Inline new-file/folder state for the explorer sidebar.  When `Some`,
    /// a temporary editable row is inserted in the tree under `parent_dir`.
    pub explorer_new_entry: Option<ExplorerNewEntryState>,
}

impl Engine {
    pub fn new() -> Self {
        let mut buffer_manager = BufferManager::new();
        let buffer_id = buffer_manager.create();

        let window_id = WindowId(1);
        let window = Window::new(window_id, buffer_id);
        let mut windows = HashMap::new();
        windows.insert(window_id, window);

        let tab = Tab::new(TabId(1), window_id);

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let mut engine = Self {
            buffer_manager,
            windows,
            editor_groups: {
                let mut m = HashMap::new();
                m.insert(GroupId(0), EditorGroup::new(tab));
                m
            },
            active_group: GroupId(0),
            prev_active_group: None,
            group_layout: GroupLayout::leaf(GroupId(0)),
            next_group_id: 1,
            next_window_id: 2,
            next_tab_id: 2,
            tab_drag: None,
            tab_drag_mouse: None,
            tab_drop_zone: DropZone::None,
            preview_buffer_id: None,
            mode: Mode::Normal,
            command_buffer: String::new(),
            command_cursor: 0,
            wildmenu_items: Vec::new(),
            wildmenu_selected: None,
            wildmenu_original: String::new(),
            message: String::new(),
            search_query: String::new(),
            search_matches: Vec::new(),
            search_index: None,
            search_direction: SearchDirection::Forward,
            search_start_cursor: None,
            replace_text: String::new(),
            replace_flags: String::new(),
            pending_key: None,
            user_keymaps: Vec::new(),
            keymap_buf: Vec::new(),
            keymap_replaying: false,
            window_nav_overflow: None,
            registers: HashMap::new(),
            selected_register: None,
            marks: HashMap::new(),
            visual_anchor: None,
            count: None,
            last_find: None,
            pending_operator: None,
            pending_find_operator: None,
            pending_text_object: None,
            last_change: None,
            insert_text_buffer: String::new(),
            virtual_replace: false,
            pending_change_motion: None,
            settings: {
                // Ensure settings.json exists with defaults
                Settings::ensure_exists().ok();
                Settings::load()
            },
            session: SessionState::load(),
            history: HistoryState::load(),
            command_history_index: None,
            command_typing_buffer: String::new(),
            history_search_active: false,
            history_search_query: String::new(),
            history_search_index: None,
            search_history_index: None,
            search_typing_buffer: String::new(),
            macro_recording: None,
            recording_buffer: Vec::new(),
            macro_playback_queue: VecDeque::new(),
            last_macro_register: None,
            macro_recursion_depth: 0,
            git_branch: {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                git::current_branch(&cwd)
            },
            scroll_bind_pairs: Vec::new(),
            completion_candidates: Vec::new(),
            completion_idx: None,
            completion_start_col: 0,
            completion_display_only: false,
            project_search_query: String::new(),
            project_search_results: Vec::new(),
            project_search_selected: 0,
            project_search_options: SearchOptions::default(),
            project_search_receiver: None,
            project_search_running: false,
            project_replace_text: String::new(),
            project_replace_receiver: None,
            project_replace_running: false,
            lsp_manager: None,
            lsp_diagnostics: HashMap::new(),
            lsp_hover_text: None,
            lsp_completion_active: false,
            lsp_pending_completion: None,
            lsp_pending_hover: None,
            lsp_pending_definition: None,
            lsp_pending_references: None,
            lsp_pending_implementation: None,
            lsp_pending_type_definition: None,
            lsp_pending_signature: None,
            lsp_pending_formatting: None,
            format_on_save_pending: None,
            quit_after_format_save: false,
            format_save_quit_ready: false,
            lsp_pending_rename: None,
            lsp_pending_semantic_tokens: HashMap::new(),
            lsp_signature_help: None,
            lsp_dirty_buffers: HashMap::new(),
            lsp_pending_code_action: None,
            lsp_pending_document_symbols: None,
            lsp_pending_workspace_symbols: None,
            lsp_code_action_request_ctx: None,
            lsp_code_actions: HashMap::new(),
            lsp_code_action_last_line: None,
            lsp_show_code_action_popup_pending: false,
            pending_code_action_choices: Vec::new(),
            cursor_move_pending: None,
            lsp_installing: std::collections::HashSet::new(),
            lsp_lookup_in_flight: std::collections::HashSet::new(),
            leader_partial: None,
            jump_list: Vec::new(),
            jump_list_pos: 0,
            search_word_bounded: false,
            workspace_file: None,
            workspace_root: Some(cwd.clone()),
            base_settings: None,
            explorer_has_focus: false,
            search_has_focus: false,
            sc_file_statuses: Vec::new(),
            sc_worktrees: Vec::new(),
            sc_selected: 0,
            sc_sections_expanded: [true, true, true, true],
            sc_has_focus: false,
            sc_ahead: 0,
            sc_behind: 0,
            sc_log: Vec::new(),
            sc_commit_message: String::new(),
            sc_commit_cursor: 0,
            sc_commit_input_active: false,
            sc_button_focused: None,
            sc_button_hovered: None,
            sc_branch_picker_open: false,
            sc_branch_picker_query: String::new(),
            sc_branch_picker_branches: Vec::new(),
            sc_branch_picker_selected: 0,
            sc_branch_create_mode: false,
            sc_branch_create_input: String::new(),
            sc_help_open: false,
            plugin_manager: None,
            comment_overrides: HashMap::new(),
            highlight_overrides: HashMap::new(),
            cwd,
            tab_switcher_open: false,
            tab_switcher_selected: 0,
            tab_mru: vec![(GroupId(0), 0)],
            tab_nav_history: vec![(GroupId(0), TabId(1))],
            tab_nav_index: 0,
            tab_nav_navigating: false,
            quickfix_items: Vec::new(),
            quickfix_selected: 0,
            quickfix_open: false,
            quickfix_has_focus: false,
            dap_sidebar_has_focus: false,
            picker_open: false,
            picker_source: PickerSource::Files,
            picker_query: String::new(),
            picker_all_items: Vec::new(),
            picker_items: Vec::new(),
            picker_selected: 0,
            picker_scroll_top: 0,
            picker_title: String::new(),
            picker_preview: None,
            breadcrumb_focus: false,
            breadcrumb_selected: 0,
            breadcrumb_segments: Vec::new(),
            breadcrumb_scoped_parent: None,
            diff_window_pair: None,
            diff_results: HashMap::new(),
            diff_aligned: HashMap::new(),
            diff_unchanged_hidden: false,
            sc_diff_rx: None,
            sc_diff_pending_win: None,
            diff_peek: None,
            clipboard_read: None,
            clipboard_write: None,
            mouse_drag_active: false,
            mouse_drag_origin_window: None,
            mouse_drag_word_mode: false,
            mouse_drag_word_origin: None,
            terminal_panes: Vec::new(),
            pending_install_context: None,
            pending_terminal_command: None,
            terminal_active: 0,
            terminal_open: false,
            terminal_has_focus: false,
            terminal_find_active: false,
            terminal_find_query: String::new(),
            terminal_find_selected: 0,
            terminal_find_matches: Vec::new(),
            terminal_split: false,
            terminal_split_left_cols: 0,
            menu_bar_visible: false,
            menu_open_idx: None,
            debug_toolbar_visible: false,
            dap_session_active: false,
            menu_highlighted_item: None,
            dap_manager: None,
            dap_stopped_thread: None,
            dap_breakpoints: HashMap::new(),
            dap_seq_initialize: None,
            dap_seq_launch: None,
            dap_current_line: None,
            dap_stack_frames: Vec::new(),
            dap_variables: Vec::new(),
            dap_output_lines: Vec::new(),
            dap_ansi_carry: String::new(),
            dap_active_frame: 0,
            dap_expanded_vars: HashSet::new(),
            dap_child_variables: HashMap::new(),
            dap_eval_result: None,
            dap_pending_vars_ref: 0,
            dap_primary_scope_name: String::new(),
            dap_primary_scope_ref: 0,
            dap_scope_groups: Vec::new(),
            dap_sidebar_section: DebugSidebarSection::Variables,
            dap_sidebar_selected: 0,
            dap_sidebar_scroll: [0; 4],
            dap_sidebar_section_heights: [0; 4],
            dap_watch_expressions: Vec::new(),
            dap_watch_values: Vec::new(),
            dap_launch_configs: Vec::new(),
            dap_selected_launch_config: 0,
            bottom_panel_kind: BottomPanelKind::Terminal,
            dap_pending_launch: None,
            bottom_panel_open: false,
            dap_wants_sidebar: false,
            dap_pending_watch_seqs: HashMap::new(),
            dap_pre_launch_done: false,
            dap_deferred_lang: None,
            last_jump_pos: None,
            last_edit_pos: None,
            last_insert_pos: None,
            visual_mark_start: None,
            visual_mark_end: None,
            global_marks: HashMap::new(),
            last_visual_anchor: None,
            last_visual_cursor: None,
            last_visual_mode: Mode::Normal,
            change_list: Vec::new(),
            change_list_pos: 0,
            last_inserted_text: String::new(),
            last_ex_command: None,
            last_substitute: None,
            yank_highlight: None,
            bracket_match: None,
            insert_ctrl_r_pending: false,
            insert_ctrl_g_pending: false,
            insert_ctrl_o_active: false,
            insert_ctrl_v_pending: false,
            visual_block_insert_info: None,
            force_motion_mode: None,
            extension_state: ExtensionState::load(),
            prompted_extensions: HashSet::new(),
            ext_hint_pending_name: None,
            ext_registry: registry::load_cache(),
            ext_registry_fetching: false,
            ext_registry_rx: None,
            ext_sidebar_has_focus: false,
            ext_sidebar_selected: 0,
            ext_sidebar_query: String::new(),
            ext_sidebar_sections_expanded: [true, true],
            ext_sidebar_input_active: false,
            pending_ext_remove: None,
            pending_git_remote_op: None,
            settings_has_focus: false,
            settings_selected: 0,
            settings_scroll_top: 0,
            settings_query: String::new(),
            settings_input_active: false,
            settings_editing: None,
            settings_edit_buf: String::new(),
            settings_collapsed: vec![false; super::settings::setting_categories().len()],
            ext_settings: HashMap::new(),
            ext_settings_collapsed: HashMap::new(),
            ext_settings_editing: None,
            line_annotations: HashMap::new(),
            blame_annotations_active: false,
            blame_rx: None,
            async_shell_tasks: HashMap::new(),
            ai_ghost_text: None,
            ai_ghost_alternatives: Vec::new(),
            ai_ghost_alt_idx: 0,
            ai_completion_ticks: None,
            ai_completion_rx: None,
            ai_completion_prefix_tail: String::new(),
            ai_messages: Vec::new(),
            ai_input: String::new(),
            ai_input_cursor: 0,
            ai_has_focus: false,
            ai_input_active: false,
            ai_streaming: false,
            ai_rx: None,
            ai_scroll_top: 0,
            md_preview_links: HashMap::new(),
            swap_write_needed: HashSet::new(),
            swap_last_write: std::time::Instant::now(),
            pending_swap_recovery: None,
            dialog: None,
            spell_checker: None,
            spell_suggestions: None,
            vscode_pending_ctrl_k: false,
            vscode_undo_group_open: false,
            vscode_undo_cursor: (0, 0),
            ext_panels: HashMap::new(),
            ext_panel_items: HashMap::new(),
            ext_panel_active: None,
            ext_panel_has_focus: false,
            ext_panel_selected: 0,
            ext_panel_scroll_top: 0,
            ext_panel_sections_expanded: HashMap::new(),
            ext_panel_tree_expanded: HashMap::new(),
            ext_panel_input_text: HashMap::new(),
            ext_panel_input_active: false,
            ext_panel_focus_pending: None,
            ext_panel_help_open: false,
            ext_panel_help_bindings: HashMap::new(),
            editor_hover: None,
            editor_hover_dwell: None,
            editor_hover_dismiss_at: None,
            lsp_hover_null_pos: None,
            lsp_hover_request_pos: None,
            editor_hover_has_focus: false,
            editor_hover_content: HashMap::new(),
            tab_hover_tooltip: None,
            perf_log: None,
            panel_hover: None,
            panel_hover_dwell: None,
            panel_hover_dismiss_at: None,
            panel_hover_registry: HashMap::new(),
            context_menu: None,
            diff_selected_file: None,
            pending_move: None,
            pending_delete: None,
            explorer_needs_refresh: false,
            explorer_rename: None,
            explorer_new_entry: None,
        };
        // If vscode mode is configured, start in Insert mode with menu visible
        if engine.is_vscode_mode() {
            engine.mode = Mode::Insert;
            engine.menu_bar_visible = true;
        }
        engine.rebuild_user_keymaps();
        engine.ensure_spell_checker();
        engine
    }

    /// Create an engine with a file loaded (or empty buffer for new file).
    #[cfg(test)]
    pub fn open(path: &Path) -> Self {
        let mut engine = Self::new();

        // Replace the default empty buffer with the file
        let old_buffer_id = engine.active_buffer_id();
        let _ = engine.buffer_manager.delete(old_buffer_id, true);

        match engine.buffer_manager.open_file(path) {
            Ok(buffer_id) => {
                engine
                    .buffer_manager
                    .apply_language_map(buffer_id, &engine.settings.language_map);
                // Update the window to point to the new buffer
                if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                    window.buffer_id = buffer_id;
                }
                // Restore saved cursor/scroll position from previous session
                let view = engine.restore_file_position(buffer_id);
                if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                    window.view = view;
                }
                engine.refresh_git_diff(buffer_id);
                engine.lsp_did_open(buffer_id);
                if !path.exists() {
                    engine.message = format!("\"{}\" [New File]", path.display());
                }
            }
            Err(e) => {
                engine.message = format!("Error reading {}: {}", path.display(), e);
                // Create a new empty buffer since we deleted the old one
                let buffer_id = engine.buffer_manager.create();
                if let Some(window) = engine.windows.get_mut(&engine.active_window_id()) {
                    window.buffer_id = buffer_id;
                }
            }
        }

        engine.plugin_init();
        engine
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Return the number of visual rows a buffer line of `line_char_len` characters
/// Returns true if `binary` is found anywhere on the current process PATH.
/// Walks PATH directories directly (no subprocess) so it works even when
/// the user's shell aliases or profile scripts are not sourced.
/// List custom VSCode theme names from `~/.config/vimcode/themes/*.json`.
fn list_custom_theme_names() -> Vec<String> {
    let mut names = Vec::new();
    {
        let dir = paths::vimcode_config_dir().join("themes");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
    }
    names.sort();
    names
}

fn binary_on_path(binary: &str) -> bool {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        let full = dir.join(binary);
        if full.exists() {
            super::lsp_manager::install_log(&format!(
                "[ext-check] FOUND {binary} at {}",
                full.display()
            ));
            return true;
        }
        // On Windows, also check with .exe suffix
        #[cfg(target_os = "windows")]
        if !binary.ends_with(".exe") {
            let exe = dir.join(format!("{binary}.exe"));
            if exe.exists() {
                super::lsp_manager::install_log(&format!(
                    "[ext-check] FOUND {binary}.exe at {}",
                    exe.display()
                ));
                return true;
            }
        }
    }
    super::lsp_manager::install_log(&format!(
        "[ext-check] NOT FOUND {binary} in PATH={}",
        path_var.to_string_lossy()
    ));
    false
}

// ── Auto-pair helpers ────────────────────────────────────────────────────────

/// Return the closing character for an auto-pair opener, or `None`.
fn auto_pair_closer(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

/// Return true if `ch` is a closing bracket/quote that can be skipped over.
fn is_closing_pair(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`')
}

/// Return true if `ch` is a quote character (needs smart context check).
fn is_quote_char(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

/// Convert a char index in `s` to a byte offset.
/// Returns `s.len()` if `char_idx` is at or beyond the end.
fn cmd_char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// occupies when the viewport is `viewport_cols` columns wide.
/// Always returns at least 1 (even for empty lines).
/// Duplicated from render.rs so core/ stays GTK/render-free.
fn engine_visual_rows_for_line(line_char_len: usize, viewport_cols: usize) -> usize {
    if viewport_cols == 0 {
        return 1;
    }
    line_char_len.div_ceil(viewport_cols).max(1)
}

/// Try to parse a `:norm[al][!] {keys}` command with an optional range prefix.
/// Returns `(range_str, keys)` if recognized, `None` otherwise.
/// Supported ranges: `""` (current line), `"%"` (all), `"'<,'>"` (visual), `"N,M"` (numeric, 1-based).
fn try_parse_norm(cmd: &str) -> Option<(&str, &str)> {
    // Strip optional range prefix
    let (range_str, rest) = if let Some(r) = cmd.strip_prefix("'<,'>") {
        ("'<,'>", r)
    } else if let Some(r) = cmd.strip_prefix('%') {
        ("%", r)
    } else if let Some(idx) = norm_numeric_range_end(cmd) {
        (&cmd[..idx], &cmd[idx..])
    } else {
        ("", cmd)
    };

    // Strip "norm[al][!] " keyword — trailing space is required; keys must follow
    let keys = rest
        .strip_prefix("normal! ")
        .or_else(|| rest.strip_prefix("normal "))
        .or_else(|| rest.strip_prefix("norm! "))
        .or_else(|| rest.strip_prefix("norm "))?;

    Some((range_str, keys))
}

/// Returns the byte index right after a `"N,M"` numeric range prefix, or `None`.
fn norm_numeric_range_end(cmd: &str) -> Option<usize> {
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || bytes[i] != b',' {
        return None;
    }
    i += 1; // skip ','
    let j = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == j {
        return None;
    }
    Some(i)
}

// =============================================================================
// DAP helpers

/// Walk up from `cwd` to find `Cargo.toml`, extract `[package] name`, and
/// return the path to `target/debug/{name}`. Returns an error string when the
/// binary does not yet exist (caller should tell the user to `cargo build`).
#[allow(dead_code)]
fn rust_debug_binary(cwd: &std::path::Path) -> Result<String, String> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let cargo_toml = d.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = std::fs::read_to_string(&cargo_toml)
                .map_err(|e| format!("Cannot read Cargo.toml: {e}"))?;
            // Find [package] section and parse the `name` key.
            let name = content
                .lines()
                .skip_while(|l| l.trim() != "[package]")
                .skip(1)
                .find(|l| l.trim_start().starts_with("name"))
                .and_then(|l| l.split('=').nth(1))
                .map(|v| v.trim().trim_matches('"').to_string())
                .ok_or_else(|| "Cannot find package name in Cargo.toml".to_string())?;
            let binary = d.join("target").join("debug").join(&name);
            if !binary.exists() {
                return Err(format!(
                    "Binary not found: {} — run `cargo build` first",
                    binary.display()
                ));
            }
            return Ok(binary.to_string_lossy().into_owned());
        }
        dir = d.parent();
    }
    Err("Cargo.toml not found in project tree".to_string())
}

// =============================================================================
// LCS-based two-way line diff
// =============================================================================

/// Compute per-line diff status for two sequences of lines using a standard
/// Maximum number of consecutive `Same` lines between two changed regions
/// that will be absorbed into the surrounding change for visual continuity.
/// Short "islands" of matching lines in the middle of an edit are typically
/// incidental (blank lines, common braces, imports) and fragmenting the
/// coloured block around them is confusing.
const DIFF_MERGE_SAME_THRESHOLD: usize = 1;

/// Post-process LCS diff results: short runs of `Same` lines (up to
/// [`DIFF_MERGE_SAME_THRESHOLD`]) that sit between two non-Same regions
/// are re-classified to `fill`, preventing visual fragmentation of what
/// the user perceives as a single edit.
fn merge_short_same_runs(results: &mut [DiffLine], fill: DiffLine) {
    let n = results.len();
    let mut i = 0;
    while i < n {
        if results[i] == DiffLine::Same {
            let start = i;
            while i < n && results[i] == DiffLine::Same {
                i += 1;
            }
            let run_len = i - start;
            let before_changed = start > 0 && results[start - 1] != DiffLine::Same;
            let after_changed = i < n && results[i] != DiffLine::Same;
            if before_changed && after_changed && run_len <= DIFF_MERGE_SAME_THRESHOLD {
                for r in results.iter_mut().take(i).skip(start) {
                    *r = fill;
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Myers diff algorithm — finds the Shortest Edit Script (SES) between two
/// sequences of lines.  Complexity is O((N+M)·D) where D is the edit distance,
/// which is much faster than O(N×M) LCS when the files are large but the
/// diff is small (the common case).
///
/// Returns `(status_a, status_b)` where each element corresponds to one line
/// of the respective input sequence:
/// - `DiffLine::Same`    — line is shared by both sides.
/// - `DiffLine::Removed` — line exists in `a` but not `b`.
/// - `DiffLine::Added`   — line exists in `b` but not `a`.
///
/// Build aligned diff sequences with padding so that Same lines appear at the
/// same visual row.  Walks the raw per-file diff status arrays (one entry per
/// buffer line) with two pointers and inserts `DiffLine::Padding` entries on
/// the opposite side whenever one side has Removed/Added lines that the other
/// does not.
///
/// Returns `(aligned_a, aligned_b)` — both the same length.
pub fn build_aligned_diff(
    da: &[DiffLine],
    db: &[DiffLine],
) -> (Vec<AlignedDiffEntry>, Vec<AlignedDiffEntry>) {
    let mut aligned_a = Vec::new();
    let mut aligned_b = Vec::new();
    let mut i = 0; // pointer into da (side A)
    let mut j = 0; // pointer into db (side B)

    while i < da.len() || j < db.len() {
        // Both sides have Same — they correspond to each other.
        if i < da.len() && j < db.len() && da[i] == DiffLine::Same && db[j] == DiffLine::Same {
            aligned_a.push(AlignedDiffEntry {
                source_line: Some(i),
            });
            aligned_b.push(AlignedDiffEntry {
                source_line: Some(j),
            });
            i += 1;
            j += 1;
            continue;
        }

        // Collect a change hunk: consume all non-Same lines from both sides.
        let mut removed = Vec::new();
        let mut added = Vec::new();
        while i < da.len() && da[i] != DiffLine::Same {
            removed.push(i);
            i += 1;
        }
        while j < db.len() && db[j] != DiffLine::Same {
            added.push(j);
            j += 1;
        }

        // If both sides hit Same (or end) without consuming anything,
        // treat remaining Same lines on either side as unmatched to avoid
        // an infinite loop.
        if removed.is_empty() && added.is_empty() {
            if i < da.len() {
                aligned_a.push(AlignedDiffEntry {
                    source_line: Some(i),
                });
                aligned_b.push(AlignedDiffEntry { source_line: None });
                i += 1;
            }
            if j < db.len() {
                aligned_a.push(AlignedDiffEntry { source_line: None });
                aligned_b.push(AlignedDiffEntry {
                    source_line: Some(j),
                });
                j += 1;
            }
            continue;
        }

        // Pair up removed/added lines, padding the shorter side.
        let max_len = removed.len().max(added.len());
        for k in 0..max_len {
            if k < removed.len() {
                aligned_a.push(AlignedDiffEntry {
                    source_line: Some(removed[k]),
                });
            } else {
                aligned_a.push(AlignedDiffEntry { source_line: None });
            }
            if k < added.len() {
                aligned_b.push(AlignedDiffEntry {
                    source_line: Some(added[k]),
                });
            } else {
                aligned_b.push(AlignedDiffEntry { source_line: None });
            }
        }
    }

    (aligned_a, aligned_b)
}

/// Falls back to all-Same if the edit distance exceeds `MAX_EDIT_DIST` (to
/// avoid pathological runtime on completely unrelated files).
pub fn lcs_diff(a: &[&str], b: &[&str]) -> (Vec<DiffLine>, Vec<DiffLine>) {
    let n = a.len();
    let m = b.len();
    if n == 0 && m == 0 {
        return (vec![], vec![]);
    }
    if n == 0 {
        return (vec![], vec![DiffLine::Added; m]);
    }
    if m == 0 {
        return (vec![DiffLine::Removed; n], vec![]);
    }

    // Maximum edit distance we're willing to explore.
    // Myers diff is O(N·D) in time and O(D²) in memory where D = edit distance.
    // For large files with small diffs (the common case), D is small so this is
    // fast regardless of file size.  The MAX_EDIT_DIST cap prevents blow-up when
    // two files are extremely different.
    const MAX_EDIT_DIST: usize = 2_000;
    let max_d = (n + m).min(MAX_EDIT_DIST);

    // V array indexed by k = x - y, offset so k=0 maps to index `offset`.
    let offset = max_d;
    let v_size = 2 * max_d + 1;
    let mut v = vec![0usize; v_size];

    // Store the trace of V snapshots for backtracking.
    let mut trace: Vec<Vec<usize>> = Vec::with_capacity(max_d);

    let mut found_d = None;
    'outer: for d in 0..=max_d {
        trace.push(v.clone());

        for k in (-(d as isize)..=(d as isize)).step_by(2) {
            let ki = (k + offset as isize) as usize;

            let mut x = if d == 0 {
                0
            } else if k == -(d as isize) || (k != d as isize && v[ki - 1] < v[ki + 1]) {
                v[ki + 1] // move down (insert)
            } else {
                v[ki - 1] + 1 // move right (delete)
            };

            let mut y = (x as isize - k) as usize;

            // Follow diagonal (matching lines)
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }

            v[ki] = x;

            if x >= n && y >= m {
                found_d = Some(d);
                break 'outer;
            }
        }
    }

    if found_d.is_none() {
        // Edit distance exceeded limit — fall back to all-Same.
        return (vec![DiffLine::Same; n], vec![DiffLine::Same; m]);
    }
    let d = found_d.unwrap();

    // Backtrack through the trace to build an edit script.
    // Each edit is either Insert(y_idx) or Delete(x_idx), in reverse order.
    #[derive(Clone, Copy)]
    enum Edit {
        Insert(usize), // b[y] was inserted
        Delete(usize), // a[x] was deleted
    }
    let mut edits: Vec<Edit> = Vec::with_capacity(d);
    let mut cx = n;
    let mut cy = m;

    for d_step in (1..=d).rev() {
        let v_d = &trace[d_step];
        let k = cx as isize - cy as isize;
        let ki = (k + offset as isize) as usize;

        let is_insert =
            k == -(d_step as isize) || (k != d_step as isize && v_d[ki - 1] < v_d[ki + 1]);

        let prev_k = if is_insert { k + 1 } else { k - 1 };
        let prev_ki = (prev_k + offset as isize) as usize;
        let prev_x = v_d[prev_ki];
        let prev_y = (prev_x as isize - prev_k) as usize;

        if is_insert {
            // y stepped from prev_y to prev_y+1, then diagonal to (cx, cy).
            edits.push(Edit::Insert(prev_y));
        } else {
            // x stepped from prev_x to prev_x+1, then diagonal to (cx, cy).
            edits.push(Edit::Delete(prev_x));
        }

        cx = prev_x;
        cy = prev_y;
    }
    edits.reverse();

    // Build per-line status arrays from the edit script.
    let mut da = vec![DiffLine::Same; n];
    let mut db = vec![DiffLine::Same; m];
    for edit in &edits {
        match *edit {
            Edit::Delete(x) => da[x] = DiffLine::Removed,
            Edit::Insert(y) => db[y] = DiffLine::Added,
        }
    }

    (da, db)
}

mod accessors;
mod buffers;
mod dap_ops;
mod execute;
mod ext_panel;
mod keys;
mod lsp_ops;
mod motions;
mod panels;
mod picker;
mod plugins;
mod search;
mod source_control;
mod spell_ops;
mod terminal_ops;
mod visual;
mod vscode;
mod windows;

#[cfg(test)]
mod tests;
