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
        label: "Go: Live Grep",
        shortcut: "Ctrl+G",
        vscode_shortcut: "Ctrl+Shift+F",
        action: "grep",
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
        action: "Gswitch",
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
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum PickerSource {
    Files,
    Grep,
    Commands,
    Buffers,
    RecentFiles,
    Marks,
    Registers,
    GitBranches,
    Custom(String),
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
enum PreprocKind {
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
}

/// One panel in a VSCode-style editor-group split.
/// Each group owns its own tab bar and independent tab navigation.
/// Single-group mode (the default) is identical to the previous behaviour.
#[derive(Debug, Clone)]
pub struct EditorGroup {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

impl EditorGroup {
    pub fn new(initial_tab: Tab) -> Self {
        Self {
            tabs: vec![initial_tab],
            active_tab: 0,
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

fn keybindings_reference_vim() -> String {
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

fn keybindings_reference_vscode() -> String {
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
    /// Set to true when a file move completes; backends should refresh the explorer tree
    /// and clear this flag.
    pub explorer_needs_refresh: bool,

    /// Inline rename state for the explorer sidebar.  When `Some`, the
    /// sidebar row matching `path` should render an editable text input
    /// instead of the plain filename.
    pub explorer_rename: Option<ExplorerRenameState>,
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
            cwd,
            tab_switcher_open: false,
            tab_switcher_selected: 0,
            tab_mru: vec![(GroupId(0), 0)],
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
            explorer_needs_refresh: false,
            explorer_rename: None,
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

    // =======================================================================
    // Repeat command (.)
    // =======================================================================

    fn repeat_last_change(&mut self, repeat_count: usize, changed: &mut bool) {
        let change = match &self.last_change {
            Some(c) => c.clone(),
            None => return, // No change to repeat
        };

        let final_count = if repeat_count > 1 {
            repeat_count
        } else {
            change.count
        };

        match change.op {
            ChangeOp::Insert => {
                // Repeat insert: insert the same text at current position
                self.start_undo_group();
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;

                // Insert the text final_count times
                let repeated_text = change.text.repeat(final_count);
                self.insert_with_undo(char_idx, &repeated_text);

                // Update cursor position based on inserted text
                let newlines = repeated_text.matches('\n').count();
                if newlines > 0 {
                    self.view_mut().cursor.line += newlines;
                    // Find column after last newline
                    if let Some(last_nl) = repeated_text.rfind('\n') {
                        self.view_mut().cursor.col = repeated_text[last_nl + 1..].chars().count();
                    }
                } else {
                    self.view_mut().cursor.col += repeated_text.chars().count();
                }
                self.finish_undo_group();
                *changed = true;
            }
            ChangeOp::Delete => {
                // Repeat delete with motion
                if let Some(motion) = &change.motion {
                    for _ in 0..final_count {
                        self.start_undo_group();
                        match motion {
                            Motion::Right => {
                                // Delete character(s) at cursor (like x)
                                let line = self.view().cursor.line;
                                let col = self.view().cursor.col;
                                let char_idx = self.buffer().line_to_char(line) + col;
                                let line_end = self.buffer().line_to_char(line)
                                    + self.buffer().line_len_chars(line);
                                let available = line_end - char_idx;
                                let to_delete = change.count.min(available);

                                if to_delete > 0 && char_idx < self.buffer().len_chars() {
                                    let deleted_chars: String = self
                                        .buffer()
                                        .content
                                        .slice(char_idx..char_idx + to_delete)
                                        .chars()
                                        .collect();
                                    let reg = self.active_register();
                                    self.set_register(reg, deleted_chars, false);
                                    self.clear_selected_register();
                                    self.delete_with_undo(char_idx, char_idx + to_delete);
                                    self.clamp_cursor_col();
                                    *changed = true;
                                }
                            }
                            Motion::DeleteLine => {
                                // Repeat dd
                                self.delete_lines(change.count, changed);
                            }
                            _ => {}
                        }
                        self.finish_undo_group();
                    }
                }
            }
            ChangeOp::Change => {
                // Repeat c{motion}: delete the motion range, then insert the text.
                if let Some(motion) = &change.motion {
                    for _ in 0..final_count {
                        let motion_char = match motion {
                            Motion::WordForward => 'w',
                            Motion::WordEnd => 'e',
                            Motion::WordBackward => 'b',
                            _ => continue,
                        };
                        // Reuse the same code path as the original cw/ce/cb:
                        // apply_operator_with_motion deletes the range and enters
                        // insert mode.  We then immediately insert the recorded
                        // text and return to normal mode instead.
                        let start_cursor = self.view().cursor;
                        let start_pos =
                            self.buffer().line_to_char(start_cursor.line) + start_cursor.col;
                        for _ in 0..change.count {
                            match motion_char {
                                'w' => self.move_word_forward(),
                                'b' => self.move_word_backward(),
                                'e' => self.move_word_end(),
                                _ => {}
                            }
                        }
                        let end_cursor = self.view().cursor;
                        let end_pos = self.buffer().line_to_char(end_cursor.line) + end_cursor.col;
                        self.view_mut().cursor = start_cursor;

                        let delete_end = if motion_char == 'e' {
                            (end_pos + 1).min(self.buffer().len_chars())
                        } else {
                            end_pos
                        };
                        if start_pos < delete_end {
                            self.start_undo_group();
                            self.delete_with_undo(start_pos, delete_end);
                            if !change.text.is_empty() {
                                self.insert_with_undo(start_pos, &change.text);
                                let inserted_chars = change.text.chars().count();
                                let newlines = change.text.matches('\n').count();
                                if newlines > 0 {
                                    self.view_mut().cursor.line += newlines;
                                    if let Some(last_nl) = change.text.rfind('\n') {
                                        self.view_mut().cursor.col =
                                            change.text[last_nl + 1..].chars().count();
                                    }
                                } else {
                                    self.view_mut().cursor.col += inserted_chars;
                                }
                            }
                            self.clamp_cursor_col();
                            self.finish_undo_group();
                            *changed = true;
                        }
                    }
                }
            }
            ChangeOp::Substitute => {
                // Repeat s command
                for _ in 0..final_count {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let max_col = self.get_max_cursor_col(line);
                    if max_col > 0 || self.buffer().line_len_chars(line) > 0 {
                        let char_idx = self.buffer().line_to_char(line) + col;
                        let line_end =
                            self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
                        let available = line_end - char_idx;
                        let to_delete = change.count.min(available);

                        self.start_undo_group();
                        if to_delete > 0 && char_idx < self.buffer().len_chars() {
                            self.delete_with_undo(char_idx, char_idx + to_delete);
                            *changed = true;
                        }

                        // Insert the recorded text
                        if !change.text.is_empty() {
                            self.insert_with_undo(char_idx, &change.text);
                            *changed = true;
                        }
                        self.finish_undo_group();
                    }
                }
            }
            ChangeOp::SubstituteLine | ChangeOp::DeleteToEnd | ChangeOp::ChangeToEnd => {
                // Handle other operations
            }
            ChangeOp::Replace => {
                // Repeat r command
                if let Some(replacement_char) = change.text.chars().next() {
                    for _ in 0..final_count {
                        self.start_undo_group();
                        self.replace_chars(replacement_char, change.count, changed);
                        self.finish_undo_group();
                    }
                }
            }
            ChangeOp::ToggleCase => {
                // Repeat ~ command
                for _ in 0..final_count {
                    self.toggle_case_at_cursor(change.count, changed);
                }
            }
            ChangeOp::Join => {
                // Repeat J command
                for _ in 0..final_count {
                    self.join_lines(change.count, changed);
                }
            }
            ChangeOp::Indent => {
                // Repeat >> command
                let line = self.view().cursor.line;
                for _ in 0..final_count {
                    self.indent_lines(line, change.count, changed);
                }
            }
            ChangeOp::Dedent => {
                // Repeat << command
                let line = self.view().cursor.line;
                for _ in 0..final_count {
                    self.dedent_lines(line, change.count, changed);
                }
            }
        }
    }

    /// Available commands for auto-completion
    fn available_commands() -> &'static [&'static str] {
        &[
            // File operations
            "w",
            "q",
            "q!",
            "wq",
            "wq!",
            "wa",
            "wqa",
            "qa",
            "qa!",
            "e ",
            "e!",
            "enew",
            // Buffers
            "bn",
            "bp",
            "bd",
            "b#",
            "ls",
            "buffers",
            "files",
            // Splits & tabs
            "split",
            "vsplit",
            "close",
            "only",
            "new",
            "wincmd ",
            "tabnew",
            "tabnext",
            "tabprev",
            "tabclose",
            "tabmove",
            // Search & replace
            "s/",
            "%s/",
            "noh",
            "nohlsearch",
            // Settings & config
            "set ",
            "config reload",
            "Settings",
            "Keymaps",
            "Keybindings",
            "Keybindings ",
            "colorscheme ",
            // Editor groups
            "EditorGroupSplit",
            "EditorGroupSplitDown",
            "EditorGroupClose",
            "EditorGroupFocus",
            "EditorGroupMoveTab",
            "egsp",
            "egspd",
            "egc",
            "egf",
            "egmt",
            // Netrw / file browser
            "Explore",
            "Ex",
            "Sexplore",
            "Sex",
            "Vexplore",
            "Vex",
            // Git
            "Gdiff",
            "Gd",
            "Gdiffsplit",
            "Gds",
            "Gstatus",
            "Gs",
            "Gadd",
            "Ga",
            "Gcommit",
            "Gc",
            "Gpush",
            "Gp",
            "Gblame",
            "Gb",
            "Ghs",
            "Ghunk",
            "Gpull",
            "Gfetch",
            "Gswitch",
            "GSwitch",
            "Gsw",
            "Gbranch",
            "GBranch",
            "GWorktreeAdd",
            "GWorktreeRemove",
            "DiffPeek",
            "DiffNext",
            "DiffPrev",
            "DiffToggleContext",
            // LSP
            "LspInfo",
            "LspRestart",
            "LspStop",
            "LspInstall",
            "Lformat",
            "Rename",
            "def",
            "refs",
            "hover",
            "LspImpl",
            "LspTypedef",
            "CodeAction",
            // Navigation
            "nextdiag",
            "prevdiag",
            "nexthunk",
            "prevhunk",
            "fuzzy",
            "sidebar",
            "palette",
            // DAP / Debug
            "DapInfo",
            "DapInstall",
            "DapCondition",
            "DapHitCondition",
            "DapLogMessage",
            "DapWatch",
            "DapBottomPanel",
            "DapEval",
            "DapExpand",
            "debug",
            "continue",
            "pause",
            "stop",
            "restart",
            "stepover",
            "stepin",
            "stepout",
            "brkpt",
            // Extensions
            "ExtInstall",
            "ExtList",
            "ExtEnable",
            "ExtDisable",
            "ExtRemove",
            "ExtRefresh",
            // AI
            "AI ",
            "AiClear",
            // Markdown
            "MarkdownPreview",
            "MdPreview",
            // Display / info
            "registers",
            "display",
            "marks",
            "jumps",
            "changes",
            "history",
            "echo ",
            // Diff
            "diffthis",
            "diffoff",
            "diffsplit",
            // Misc ex commands
            "sort",
            "terminal",
            "cd ",
            "make",
            "copen",
            "cn",
            "cp",
            "cc",
            "r ",
            "norm ",
            "Plugin",
            "map",
            "unmap",
        ]
    }

    /// All setting names recognized by `:set`.
    fn setting_names() -> &'static [&'static str] {
        &[
            // Boolean options (full names + aliases)
            "number",
            "nu",
            "relativenumber",
            "rnu",
            "expandtab",
            "et",
            "autoindent",
            "ai",
            "incsearch",
            "is",
            "lsp",
            "wrap",
            "hlsearch",
            "hls",
            "ignorecase",
            "ic",
            "smartcase",
            "scs",
            "cursorline",
            "cul",
            "autoread",
            "ar",
            "splitbelow",
            "sb",
            "splitright",
            "spr",
            "ai_completions",
            "formatonsave",
            "fos",
            "showhiddenfiles",
            "shf",
            "swapfile",
            "breadcrumbs",
            "autohidepanels",
            // Value options
            "tabstop",
            "ts",
            "shiftwidth",
            "sw",
            "scrolloff",
            "so",
            "colorcolumn",
            "cc",
            "textwidth",
            "tw",
            "updatetime",
            "ut",
            "mode",
            "filetype",
            "ft",
        ]
    }

    /// Find completions for partial command, including argument completion.
    fn complete_command(&self, partial: &str) -> Vec<String> {
        if partial.is_empty() {
            return Vec::new();
        }

        // Check if we're completing an argument (text after a space)
        if let Some(space_pos) = partial.find(' ') {
            let cmd_prefix = &partial[..space_pos];
            let arg_partial = partial[space_pos + 1..].trim_start();

            return match cmd_prefix {
                "set" => {
                    // Complete setting names, including "no" prefixed variants
                    let mut results: Vec<String> = Self::setting_names()
                        .iter()
                        .filter(|name| name.starts_with(arg_partial))
                        .map(|name| format!("set {name}"))
                        .collect();
                    // Also offer "no" prefixed boolean disable variants
                    for name in Self::setting_names() {
                        let no_name = format!("no{name}");
                        if no_name.starts_with(arg_partial) && !arg_partial.is_empty() {
                            results.push(format!("set {no_name}"));
                        }
                    }
                    results.sort();
                    results.dedup();
                    results
                }
                "Keybindings" | "keybindings" => ["vim", "vscode"]
                    .iter()
                    .filter(|m| m.starts_with(arg_partial))
                    .map(|m| format!("Keybindings {m}"))
                    .collect(),
                "colorscheme" => {
                    // Complete theme names
                    let mut names = vec![
                        "onedark".to_string(),
                        "gruvbox-dark".to_string(),
                        "tokyo-night".to_string(),
                        "solarized-dark".to_string(),
                        "vscode-dark".to_string(),
                        "vscode-light".to_string(),
                        "gruvbox".to_string(),
                        "tokyonight".to_string(),
                        "solarized".to_string(),
                    ];
                    names.extend(list_custom_theme_names());
                    names.sort();
                    names.dedup();
                    names
                        .into_iter()
                        .filter(|name| name.starts_with(arg_partial))
                        .map(|name| format!("colorscheme {name}"))
                        .collect()
                }
                _ => {
                    // For other commands with trailing space in available_commands,
                    // fall through to prefix matching
                    Self::available_commands()
                        .iter()
                        .filter(|cmd| cmd.starts_with(partial))
                        .map(|s| s.to_string())
                        .collect()
                }
            };
        }

        // First-word completion
        Self::available_commands()
            .iter()
            .filter(|cmd| cmd.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }

    /// Find common prefix of strings
    fn find_common_prefix(strings: &[String]) -> String {
        if strings.is_empty() {
            return String::new();
        }

        let first = &strings[0];
        let mut common = String::new();

        for (i, ch) in first.chars().enumerate() {
            if strings.iter().all(|s| s.chars().nth(i) == Some(ch)) {
                common.push(ch);
            } else {
                break;
            }
        }

        common
    }

    // ─── Workspace / Open Folder ──────────────────────────────────────────────

    /// Open a folder as the new working directory.  Clears all buffers/tabs,
    /// resets the explorer root, and loads any per-project session state.
    pub fn open_folder(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Save current per-workspace session before switching
        if let Some(ref root) = self.workspace_root.clone() {
            self.save_session_for_workspace(root);
        }

        // Restore user settings baseline before applying any new folder overlay
        if let Some(base) = self.base_settings.take() {
            self.settings = *base;
        }

        // Check if the new folder has a per-folder settings file to apply as overlay
        let folder_settings_path = canonical.join(".vimcode").join("settings.json");
        if folder_settings_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&folder_settings_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(obj) = json.as_object() {
                        // Save baseline before overlay
                        self.base_settings = Some(Box::new(self.settings.clone()));
                        for (key, value) in obj {
                            let arg = match value {
                                serde_json::Value::Bool(b) => {
                                    if *b {
                                        key.clone()
                                    } else {
                                        format!("no{}", key)
                                    }
                                }
                                serde_json::Value::Number(n) => format!("{}={}", key, n),
                                serde_json::Value::String(s) => format!("{}={}", key, s),
                                _ => continue,
                            };
                            self.settings.parse_set_option(&arg).ok();
                        }
                    }
                }
            }
        }

        // Delete swap files for all current buffers before discarding them.
        self.cleanup_all_swaps();

        // Clear all existing buffers and tabs, reset to single empty window
        self.buffer_manager = super::buffer_manager::BufferManager::new();
        let buffer_id = self.buffer_manager.create();
        let window_id = super::window::WindowId(self.next_window_id);
        self.next_window_id += 1;
        let window = super::window::Window::new(window_id, buffer_id);
        self.windows.clear();
        self.windows.insert(window_id, window);
        let tab = super::tab::Tab::new(super::tab::TabId(self.next_tab_id), window_id);
        self.next_tab_id += 1;
        self.editor_groups.clear();
        let gid = GroupId(0);
        self.editor_groups.insert(gid, EditorGroup::new(tab));
        self.active_group = gid;
        self.group_layout = GroupLayout::leaf(gid);
        self.next_group_id = 1;
        self.mode = Mode::Normal;

        // Update cwd + workspace root + process working directory
        self.cwd = canonical.clone();
        self.workspace_root = Some(canonical.clone());
        let _ = std::env::set_current_dir(&canonical);

        // Update git branch
        self.git_branch = git::current_branch(&canonical);

        // Load per-project session (restores open files + positions)
        let ws_session = SessionState::load_for_workspace(&canonical);
        // Restore open files from session
        let open_files: Vec<PathBuf> = ws_session.open_files.clone();
        let active_file = ws_session.active_file.clone();
        // Merge relevant session fields
        self.session.file_positions = ws_session.file_positions;
        // Add to recent workspaces in global session
        self.session.add_recent_workspace(&canonical);

        // Re-open session files
        for fp in &open_files {
            self.open_file_in_tab(fp);
        }
        // Focus the previously active file
        if let Some(ref af) = active_file {
            self.open_file_in_tab(af);
        }

        self.message = format!("Opened folder: {}", canonical.display());
    }

    /// Parse and load a `.vimcode-workspace` JSON file.
    pub fn open_workspace(&mut self, ws_path: &Path) {
        let content = match std::fs::read_to_string(ws_path) {
            Ok(c) => c,
            Err(e) => {
                self.message = format!("Cannot read workspace: {}", e);
                return;
            }
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                self.message = format!("Invalid workspace JSON: {}", e);
                return;
            }
        };

        // Resolve folder path relative to workspace file
        let ws_dir = ws_path.parent().unwrap_or(Path::new("."));
        let folder_rel = json
            .get("folders")
            .and_then(|f| f.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or(".");
        let folder_path = ws_dir.join(folder_rel);
        self.workspace_file = Some(ws_path.to_path_buf());

        // Apply any settings overrides from workspace
        if let Some(settings_obj) = json.get("settings").and_then(|s| s.as_object()) {
            // Save baseline settings before applying workspace overlay (once only)
            if self.base_settings.is_none() {
                self.base_settings = Some(Box::new(self.settings.clone()));
            }
            for (key, value) in settings_obj {
                let arg = match value {
                    serde_json::Value::Bool(b) => {
                        if *b {
                            key.clone()
                        } else {
                            format!("no{}", key)
                        }
                    }
                    serde_json::Value::Number(n) => format!("{}={}", key, n),
                    serde_json::Value::String(s) => format!("{}={}", key, s),
                    _ => continue,
                };
                self.settings.parse_set_option(&arg).ok();
            }
        }

        self.open_folder(&folder_path);
        self.message = format!("Workspace loaded: {}", ws_path.display());
    }

    /// Write a `.vimcode-workspace` file at the given path with the current folder.
    /// Open or create a workspace in the directory of the currently active file.
    /// If a `.vimcode-workspace` file already exists there, open it; otherwise
    /// create one and then open the folder.
    pub fn open_workspace_from_file(&mut self) {
        let buf_id = self.active_window().buffer_id;
        let dir = self
            .buffer_manager
            .get(buf_id)
            .and_then(|bs| bs.file_path.as_ref())
            .and_then(|fp| fp.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone());
        let ws_path = dir.join(".vimcode-workspace");
        if ws_path.exists() {
            self.open_workspace(&ws_path);
        } else {
            self.save_workspace_as(&ws_path);
            self.open_folder(&dir);
        }
    }

    pub fn save_workspace_as(&mut self, ws_path: &Path) {
        let folder_path = if let Some(parent) = ws_path.parent() {
            // Make folder path relative to workspace file location
            let canonical_cwd = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
            let canonical_parent = parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf());
            if canonical_cwd == canonical_parent {
                ".".to_string()
            } else {
                canonical_cwd.to_string_lossy().into_owned()
            }
        } else {
            ".".to_string()
        };

        let ws = serde_json::json!({
            "version": 1,
            "folders": [{"path": folder_path}],
            "settings": {}
        });
        match std::fs::write(
            ws_path,
            serde_json::to_string_pretty(&ws).unwrap_or_default(),
        ) {
            Ok(()) => {
                self.workspace_file = Some(ws_path.to_path_buf());
                self.message = format!("Workspace saved: {}", ws_path.display());
            }
            Err(e) => {
                self.message = format!("Cannot save workspace: {}", e);
            }
        }
    }

    /// Save per-workspace session state (open files, cursor positions).
    pub fn save_session_for_workspace(&self, root: &Path) {
        let mut ws_session = SessionState::default();

        // Collect open file paths per group (by iterating each group's tabs).
        let files_for_group = |group: &EditorGroup| -> Vec<PathBuf> {
            let mut files: Vec<PathBuf> = Vec::new();
            for tab in &group.tabs {
                if let Some(window) = self.windows.get(&tab.active_window) {
                    if let Some(bs) = self.buffer_manager.get(window.buffer_id) {
                        if let Some(ref fp) = bs.file_path {
                            if !files.contains(fp) {
                                files.push(fp.clone());
                            }
                        }
                    }
                }
            }
            files
        };

        let group_ids = self.group_layout.group_ids();
        if let Some(gid) = group_ids.first() {
            if let Some(group) = self.editor_groups.get(gid) {
                ws_session.open_files = files_for_group(group);
            }
        }
        if group_ids.len() >= 2 {
            if let Some(group) = self.editor_groups.get(&group_ids[1]) {
                ws_session.open_files_group1 = files_for_group(group);
            }
        }
        ws_session.active_file = self.file_path().cloned();
        ws_session.file_positions = self.session.file_positions.clone();
        // Save active_group as index position in leaf order for backward compat
        ws_session.active_group = group_ids
            .iter()
            .position(|&id| id == self.active_group)
            .unwrap_or(0);
        // For backward-compat, extract direction and ratio from root split
        // (old format only supported a single split).
        if let GroupLayout::Split {
            direction, ratio, ..
        } = &self.group_layout
        {
            ws_session.group_split_direction = match direction {
                SplitDirection::Vertical => 0,
                SplitDirection::Horizontal => 1,
            };
            ws_session.group_split_ratio = *ratio;
        }
        // Save the full recursive tree layout (new format).
        ws_session.group_layout = Some(self.build_session_group_layout(&self.group_layout));
        ws_session.save_for_workspace(root).ok();
    }

    /// Recursively convert the engine's GroupLayout tree into a SessionGroupLayout
    /// for serialization, collecting each leaf group's open file paths.
    fn build_session_group_layout(&self, layout: &GroupLayout) -> SessionGroupLayout {
        match layout {
            GroupLayout::Leaf(gid) => {
                let files = self
                    .editor_groups
                    .get(gid)
                    .map(|group| {
                        let mut files: Vec<PathBuf> = Vec::new();
                        for tab in &group.tabs {
                            if let Some(window) = self.windows.get(&tab.active_window) {
                                if let Some(bs) = self.buffer_manager.get(window.buffer_id) {
                                    if let Some(ref fp) = bs.file_path {
                                        if !files.contains(fp) {
                                            files.push(fp.clone());
                                        }
                                    }
                                }
                            }
                        }
                        files
                    })
                    .unwrap_or_default();
                SessionGroupLayout::Leaf { files }
            }
            GroupLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => SessionGroupLayout::Split {
                direction: match direction {
                    SplitDirection::Vertical => 0,
                    SplitDirection::Horizontal => 1,
                },
                ratio: *ratio,
                first: Box::new(self.build_session_group_layout(first)),
                second: Box::new(self.build_session_group_layout(second)),
            },
        }
    }

    /// Recursively restore groups from a SessionGroupLayout tree.
    /// Returns the reconstructed GroupLayout tree.
    fn restore_session_group_layout(&mut self, session_layout: &SessionGroupLayout) -> GroupLayout {
        match session_layout {
            SessionGroupLayout::Leaf { files } => {
                let gid = self.new_group_id();
                // Create the group with the first file (or a scratch tab if no files).
                let valid: Vec<&PathBuf> = files.iter().filter(|p| p.exists()).collect();
                if valid.is_empty() {
                    // Empty group: create a fresh scratch buffer (don't rely on active_group chain).
                    let wid = self.new_window_id();
                    let buf_id = self.buffer_manager.create();
                    let w = Window::new(wid, buf_id);
                    self.windows.insert(wid, w);
                    let tid = self.new_tab_id();
                    let tab = Tab::new(tid, wid);
                    self.editor_groups.insert(gid, EditorGroup::new(tab));
                } else {
                    // Open files in this group's tabs.
                    let mut first = true;
                    for path in &valid {
                        let wid = self.new_window_id();
                        let buf_id = self
                            .buffer_manager
                            .open_file(path)
                            .unwrap_or_else(|_| self.buffer_manager.create());
                        let mut w = Window::new(wid, buf_id);
                        let view = self.restore_file_position(buf_id);
                        w.view = view;
                        self.windows.insert(wid, w);
                        let tid = self.new_tab_id();
                        let tab = Tab::new(tid, wid);
                        if first {
                            self.editor_groups.insert(gid, EditorGroup::new(tab));
                            first = false;
                        } else if let Some(group) = self.editor_groups.get_mut(&gid) {
                            group.tabs.push(tab);
                        }
                    }
                }
                GroupLayout::Leaf(gid)
            }
            SessionGroupLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let dir = if *direction == 1 {
                    SplitDirection::Horizontal
                } else {
                    SplitDirection::Vertical
                };
                let first_layout = self.restore_session_group_layout(first);
                let second_layout = self.restore_session_group_layout(second);
                GroupLayout::Split {
                    direction: dir,
                    ratio: *ratio,
                    first: Box::new(first_layout),
                    second: Box::new(second_layout),
                }
            }
        }
    }

    // ─── User keymaps ────────────────────────────────────────────────────────

    /// Rebuild the parsed user_keymaps cache from settings.keymaps.
    /// Call after loading or changing settings.
    pub fn rebuild_user_keymaps(&mut self) {
        self.user_keymaps = self
            .settings
            .keymaps
            .iter()
            .filter_map(|s| parse_keymap_def(s))
            .collect();
    }

    /// Check user keymaps for the current keypress. Returns `Some(action)` if
    /// an exact match was found, `None` to fall through to built-in handling.
    /// Handles multi-key sequences by buffering keypresses.
    fn try_user_keymap(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) -> Option<EngineAction> {
        if self.keymap_replaying || self.user_keymaps.is_empty() {
            return None;
        }

        let mode_str = if self.is_vscode_mode() {
            // VSCode mode has no modal distinction; "n" keymaps apply.
            "n"
        } else {
            match self.mode {
                Mode::Normal => "n",
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock => "v",
                Mode::Insert => "i",
                Mode::Command => "c",
                _ => return None,
            }
        };

        let encoded = encode_keypress(key_name, unicode, ctrl);
        self.keymap_buf.push(encoded);

        let mut exact_match_action = None;
        let mut has_prefix = false;

        for km in &self.user_keymaps {
            if km.mode != mode_str {
                continue;
            }
            if km.keys == self.keymap_buf {
                exact_match_action = Some(km.action.clone());
            } else if km.keys.len() > self.keymap_buf.len()
                && km.keys[..self.keymap_buf.len()] == self.keymap_buf[..]
            {
                has_prefix = true;
            }
        }

        if let Some(action) = exact_match_action {
            self.keymap_buf.clear();
            let count = self.take_count();
            // Substitute {count} in the action, or append count as argument
            let cmd = if action.contains("{count}") {
                action.replace("{count}", &count.to_string())
            } else if count > 1 {
                format!("{action} {count}")
            } else {
                action
            };
            *changed = true;
            return Some(self.execute_command(&cmd));
        }

        if has_prefix {
            // More keys needed — consume this keypress
            return Some(EngineAction::None);
        }

        // No match and no prefix. Replay buffered keys.
        let buf: Vec<String> = self.keymap_buf.drain(..).collect();
        if buf.len() <= 1 {
            // Single key, no match — fall through to built-in handling
            return None;
        }

        // Multi-key sequence that didn't match any keymap: replay all keys
        self.keymap_replaying = true;
        let mut last_action = EngineAction::None;
        for encoded_key in buf {
            let (rk_name, rk_unicode, rk_ctrl) = decode_keypress(&encoded_key);
            last_action = self.handle_key(&rk_name, rk_unicode, rk_ctrl);
        }
        self.keymap_replaying = false;
        Some(last_action)
    }

    /// Try to run a named plugin command. Returns `true` if the command was found.
    pub fn plugin_run_command(&mut self, name: &str, args: &str) -> bool {
        if !self.settings.plugins_enabled {
            return false;
        }
        let pm = match self.plugin_manager.take() {
            Some(p) => p,
            None => return false,
        };
        let ctx = self.make_plugin_ctx(false);
        let (found, ctx) = pm.call_command(name, args, ctx);
        self.plugin_manager = Some(pm);
        self.apply_plugin_ctx(ctx);
        found
    }

    /// Try to run a plugin keymap. Returns `true` if a mapping was found and executed.
    pub fn plugin_run_keymap(&mut self, mode: &str, key: &str) -> bool {
        if !self.settings.plugins_enabled {
            return false;
        }
        let pm = match self.plugin_manager.take() {
            Some(p) => p,
            None => return false,
        };
        let ctx = self.make_plugin_ctx(false);
        let (found, ctx) = pm.call_keymap(mode, key, ctx);
        self.plugin_manager = Some(pm);
        self.apply_plugin_ctx(ctx);
        found
    }

    // =======================================================================
    // Bracket navigation ([ and ] commands)
    // =======================================================================

    /// Jump to next section start (]] or next section end (][).
    /// `end_section`: false = start ('{' in column 0), true = end ('}' in column 0).
    /// In LaTeX buffers: ]] jumps to next \section/\chapter/\subsection/\subsubsection,
    /// ][ jumps to next \end{...}.
    fn jump_section_forward(&mut self, end_section: bool) {
        if self.is_latex_buffer() {
            self.jump_latex_section_forward(end_section);
            return;
        }
        let target_char = if end_section { '}' } else { '{' };
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        for line in start..total {
            let line_start = self.buffer().line_to_char(line);
            if self.buffer().line_len_chars(line) > 0
                && self.buffer().content.char(line_start) == target_char
            {
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = 0;
                return;
            }
        }
    }

    /// Jump to previous section start ([[) or previous section end (][]).
    /// In LaTeX buffers: [[ jumps to previous \section/etc., [] jumps to previous \end{}.
    fn jump_section_backward(&mut self, end_section: bool) {
        if self.is_latex_buffer() {
            self.jump_latex_section_backward(end_section);
            return;
        }
        let target_char = if end_section { '}' } else { '{' };
        let cur = self.view().cursor.line;
        for line in (0..cur).rev() {
            let line_start = self.buffer().line_to_char(line);
            if self.buffer().line_len_chars(line) > 0
                && self.buffer().content.char(line_start) == target_char
            {
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = 0;
                return;
            }
        }
    }

    /// Jump to next method start (]m) — finds next '{' that starts a block.
    /// In LaTeX buffers: ]m jumps to next \begin{...}.
    fn jump_method_start_forward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_forward(false);
            return;
        }
        let total_chars = self.buffer().len_chars();
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        let mut pos = cur_pos + 1;
        while pos < total_chars {
            if self.buffer().content.char(pos) == '{' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            pos += 1;
        }
    }

    /// Jump to previous method start ([m).
    /// In LaTeX buffers: [m jumps to previous \begin{...}.
    fn jump_method_start_backward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_backward(false);
            return;
        }
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        if cur_pos == 0 {
            return;
        }
        let mut pos = cur_pos - 1;
        loop {
            if self.buffer().content.char(pos) == '{' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
    }

    /// Jump to next method end (]M) — finds next '}'.
    /// In LaTeX buffers: ]M jumps to next \end{...}.
    fn jump_method_end_forward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_forward(true);
            return;
        }
        let total_chars = self.buffer().len_chars();
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        let mut pos = cur_pos + 1;
        while pos < total_chars {
            if self.buffer().content.char(pos) == '}' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            pos += 1;
        }
    }

    /// Jump to previous method end ([M).
    /// In LaTeX buffers: [M jumps to previous \end{...}.
    fn jump_method_end_backward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_backward(true);
            return;
        }
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        if cur_pos == 0 {
            return;
        }
        let mut pos = cur_pos - 1;
        loop {
            if self.buffer().content.char(pos) == '}' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
    }

    // --- LaTeX-specific motion helpers ---

    /// LaTeX section commands to match for ]] / [[ jumps.
    const LATEX_SECTION_COMMANDS: &'static [&'static str] = &[
        "\\part",
        "\\chapter",
        "\\section",
        "\\subsection",
        "\\subsubsection",
        "\\paragraph",
        "\\subparagraph",
    ];

    /// Jump forward to next LaTeX section command (]]) or \end{} (][).
    fn jump_latex_section_forward(&mut self, end_section: bool) {
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        for line in start..total {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let trimmed = line_text.trim_start();
            if end_section {
                if trimmed.starts_with("\\end{") {
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = 0;
                    return;
                }
            } else {
                for cmd in Self::LATEX_SECTION_COMMANDS {
                    if let Some(after) = trimmed.strip_prefix(cmd) {
                        if after.starts_with('{') || after.starts_with('*') || after.is_empty() {
                            self.view_mut().cursor.line = line;
                            self.view_mut().cursor.col = 0;
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Jump backward to previous LaTeX section command ([[) or \end{} ([]).
    fn jump_latex_section_backward(&mut self, end_section: bool) {
        let cur = self.view().cursor.line;
        for line in (0..cur).rev() {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let trimmed = line_text.trim_start();
            if end_section {
                if trimmed.starts_with("\\end{") {
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = 0;
                    return;
                }
            } else {
                for cmd in Self::LATEX_SECTION_COMMANDS {
                    if let Some(after) = trimmed.strip_prefix(cmd) {
                        if after.starts_with('{') || after.starts_with('*') || after.is_empty() {
                            self.view_mut().cursor.line = line;
                            self.view_mut().cursor.col = 0;
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Jump forward to next \begin{} (is_end=false) or \end{} (is_end=true).
    fn jump_latex_env_forward(&mut self, is_end: bool) {
        let needle = if is_end { "\\end{" } else { "\\begin{" };
        let total = self.buffer().len_lines();
        let start_line = self.view().cursor.line;
        let start_col = self.view().cursor.col;
        for line in start_line..total {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let search_from = if line == start_line { start_col + 1 } else { 0 };
            if search_from < line_text.len() {
                if let Some(rel) = line_text[search_from..].find(needle) {
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = search_from + rel;
                    return;
                }
            }
        }
    }

    /// Jump backward to previous \begin{} (is_end=false) or \end{} (is_end=true).
    fn jump_latex_env_backward(&mut self, is_end: bool) {
        let needle = if is_end { "\\end{" } else { "\\begin{" };
        let start_line = self.view().cursor.line;
        let start_col = self.view().cursor.col;
        for line in (0..=start_line).rev() {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let search_end = if line == start_line {
                start_col
            } else {
                line_text.len()
            };
            if let Some(pos) = line_text[..search_end].rfind(needle) {
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos;
                return;
            }
        }
    }

    /// Jump forward to next unmatched close bracket (]} or ])).
    fn jump_unmatched_forward(&mut self, open: char, close: char) {
        let total_chars = self.buffer().len_chars();
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        let mut pos = cur_pos + 1;
        let mut depth: i32 = 0;
        while pos < total_chars {
            let ch = self.buffer().content.char(pos);
            if ch == open {
                depth += 1;
            } else if ch == close {
                if depth == 0 {
                    let line = self.buffer().content.char_to_line(pos);
                    let line_start = self.buffer().line_to_char(line);
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = pos - line_start;
                    return;
                }
                depth -= 1;
            }
            pos += 1;
        }
    }

    /// Jump backward to previous unmatched open bracket ([{ or [().
    fn jump_unmatched_backward(&mut self, open: char, close: char) {
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        if cur_pos == 0 {
            return;
        }
        let mut pos = cur_pos - 1;
        let mut depth: i32 = 0;
        loop {
            let ch = self.buffer().content.char(pos);
            if ch == close {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    let line = self.buffer().content.char_to_line(pos);
                    let line_start = self.buffer().line_to_char(line);
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = pos - line_start;
                    return;
                }
                depth -= 1;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
    }

    // =======================================================================
    // Window resize (CTRL-W +/-/</>=/|/_)
    // =======================================================================

    /// Resize the window's parent split by delta steps.
    /// `direction`: which split direction to look for (Horizontal for +/-, Vertical for </>).
    /// `increase`: true = make active group bigger, false = smaller.
    fn resize_window_split(&mut self, direction: SplitDirection, increase: bool, count: usize) {
        let delta_per_step = 0.05;
        if let Some((split_idx, split_dir, is_first)) =
            self.group_layout.parent_split_of(self.active_group)
        {
            if split_dir == direction {
                // Active group is in first child → increasing ratio makes it bigger
                let delta = if (is_first && increase) || (!is_first && !increase) {
                    delta_per_step * count as f64
                } else {
                    -(delta_per_step * count as f64)
                };
                self.group_layout.adjust_ratio_at_index(split_idx, delta);
            }
        }
    }

    /// Equalize all split ratios to 0.5.
    fn equalize_splits(&mut self) {
        self.group_layout.set_all_ratios(0.5);
    }

    /// Maximize window in a given direction (CTRL-W _ for height, CTRL-W | for width).
    fn maximize_window_split(&mut self, direction: SplitDirection) {
        if let Some((split_idx, split_dir, is_first)) =
            self.group_layout.parent_split_of(self.active_group)
        {
            if split_dir == direction {
                let ratio = if is_first { 0.9 } else { 0.1 };
                self.group_layout.set_ratio_at_index(split_idx, ratio);
            }
        }
    }

    /// Execute a window command by character (`:wincmd {char}` and Ctrl-W {char}).
    fn execute_wincmd(&mut self, ch: char, count: usize) -> EngineAction {
        match ch {
            // Focus
            'h' => self.focus_window_direction(SplitDirection::Vertical, false),
            'j' => self.focus_window_direction(SplitDirection::Horizontal, true),
            'k' => self.focus_window_direction(SplitDirection::Horizontal, false),
            'l' => self.focus_window_direction(SplitDirection::Vertical, true),
            'w' | 'W' => self.focus_next_window(),
            'p' => {
                if let Some(prev) = self.prev_active_group {
                    if self.editor_groups.contains_key(&prev) {
                        let cur = self.active_group;
                        self.active_group = prev;
                        self.prev_active_group = Some(cur);
                    }
                }
            }
            't' => {
                if let Some(first) = self.group_layout.nth_leaf(0) {
                    if first != self.active_group {
                        self.prev_active_group = Some(self.active_group);
                    }
                    self.active_group = first;
                }
            }
            'b' => {
                let ids = self.group_layout.group_ids();
                if let Some(&last) = ids.last() {
                    if last != self.active_group {
                        self.prev_active_group = Some(self.active_group);
                    }
                    self.active_group = last;
                }
            }
            // Move
            'H' => self.move_window_to_edge(SplitDirection::Vertical, false),
            'J' => self.move_window_to_edge(SplitDirection::Horizontal, true),
            'K' => self.move_window_to_edge(SplitDirection::Horizontal, false),
            'L' => self.move_window_to_edge(SplitDirection::Vertical, true),
            'T' => self.move_window_to_new_group(),
            'x' => self.exchange_windows(),
            'r' => self.rotate_windows(true),
            'R' => self.rotate_windows(false),
            // Split / Close
            's' | 'S' => self.split_window(SplitDirection::Horizontal, None),
            'v' | 'V' => self.split_window(SplitDirection::Vertical, None),
            'c' | 'C' => {
                self.close_window();
            }
            'q' => {
                self.close_window();
            }
            'o' | 'O' => self.close_other_windows(),
            'n' => {
                let _ = self.execute_command("new");
            }
            // Editor groups
            'e' => self.open_editor_group(SplitDirection::Vertical),
            'E' => self.open_editor_group(SplitDirection::Horizontal),
            // Resize (count-aware)
            '+' => self.resize_window_split(SplitDirection::Horizontal, true, count),
            '-' => self.resize_window_split(SplitDirection::Horizontal, false, count),
            '>' => self.resize_window_split(SplitDirection::Vertical, true, count),
            '<' => self.resize_window_split(SplitDirection::Vertical, false, count),
            '=' => self.equalize_splits(),
            '_' => self.maximize_window_split(SplitDirection::Horizontal),
            '|' => self.maximize_window_split(SplitDirection::Vertical),
            // Composite
            'f' => {
                if let Some(path) = self.file_path_under_cursor() {
                    let abs_path = if path.is_absolute() {
                        path
                    } else {
                        self.cwd.join(&path)
                    };
                    self.split_window(SplitDirection::Horizontal, None);
                    return EngineAction::OpenFile(abs_path);
                } else {
                    self.message = "No file path under cursor".to_string();
                }
            }
            'd' => {
                self.split_window(SplitDirection::Horizontal, None);
                self.push_jump_location();
                self.lsp_request_definition();
            }
            _ => {
                self.message = format!("Unknown wincmd: {}", ch);
            }
        }
        EngineAction::None
    }

    /// Ctrl-W H/J/K/L: move current window to far edge.
    /// Creates a new editor group at the edge of the entire layout.
    fn move_window_to_edge(&mut self, direction: SplitDirection, forward: bool) {
        // Only meaningful with multiple groups
        let groups = self.group_layout.group_ids();
        if groups.len() <= 1 {
            // With a single group, split the group layout at the root
            let buf_id = self.active_buffer_id();
            let view_clone = self.view().clone();
            // Close current window if possible
            let could_close = self.close_window();
            if !could_close {
                return; // Last window, can't move
            }
            // Create new group at the edge
            let new_win_id = self.new_window_id();
            let mut new_win = Window::new(new_win_id, buf_id);
            new_win.view = view_clone;
            self.windows.insert(new_win_id, new_win);
            let tab = Tab::new(self.new_tab_id(), new_win_id);
            let new_gid = self.new_group_id();
            self.editor_groups.insert(new_gid, EditorGroup::new(tab));
            // Wrap the existing layout in a split with the new group at the desired edge
            let old_layout =
                std::mem::replace(&mut self.group_layout, GroupLayout::leaf(GroupId(0)));
            self.group_layout = if forward {
                GroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(old_layout),
                    second: Box::new(GroupLayout::leaf(new_gid)),
                }
            } else {
                GroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(GroupLayout::leaf(new_gid)),
                    second: Box::new(old_layout),
                }
            };
            self.prev_active_group = Some(self.active_group);
            self.active_group = new_gid;
            return;
        }
        // Multiple groups: remove window, create new group at edge
        let buf_id = self.active_buffer_id();
        let view_clone = self.view().clone();
        let could_close = self.close_window();
        if !could_close {
            return;
        }
        let new_win_id = self.new_window_id();
        let mut new_win = Window::new(new_win_id, buf_id);
        new_win.view = view_clone;
        self.windows.insert(new_win_id, new_win);
        let tab = Tab::new(self.new_tab_id(), new_win_id);
        let new_gid = self.new_group_id();
        self.editor_groups.insert(new_gid, EditorGroup::new(tab));
        let old_layout = std::mem::replace(&mut self.group_layout, GroupLayout::leaf(GroupId(0)));
        self.group_layout = if forward {
            GroupLayout::Split {
                direction,
                ratio: 0.5,
                first: Box::new(old_layout),
                second: Box::new(GroupLayout::leaf(new_gid)),
            }
        } else {
            GroupLayout::Split {
                direction,
                ratio: 0.5,
                first: Box::new(GroupLayout::leaf(new_gid)),
                second: Box::new(old_layout),
            }
        };
        self.prev_active_group = Some(self.active_group);
        self.active_group = new_gid;
    }

    /// Ctrl-W T: move current window to a new editor group.
    fn move_window_to_new_group(&mut self) {
        // Only meaningful with multiple windows in the current tab
        let tab = self.active_tab();
        if tab.layout.is_single_window() && self.active_group().tabs.len() == 1 {
            self.message = "Already the only window".to_string();
            return;
        }
        let buf_id = self.active_buffer_id();
        let view_clone = self.view().clone();
        let could_close = self.close_window();
        if !could_close {
            return;
        }
        // Open a new editor group with that buffer
        let new_win_id = self.new_window_id();
        let mut new_win = Window::new(new_win_id, buf_id);
        new_win.view = view_clone;
        self.windows.insert(new_win_id, new_win);
        let tab = Tab::new(self.new_tab_id(), new_win_id);
        let new_gid = self.new_group_id();
        self.editor_groups.insert(new_gid, EditorGroup::new(tab));
        self.group_layout
            .split_at(self.active_group, SplitDirection::Vertical, new_gid, false);
        self.prev_active_group = Some(self.active_group);
        self.active_group = new_gid;
    }

    /// Ctrl-W x: exchange current window with next window in the same tab.
    fn exchange_windows(&mut self) {
        let tab = self.active_tab();
        let ids = tab.layout.window_ids();
        if ids.len() < 2 {
            return;
        }
        let current_id = tab.active_window;
        let current_idx = ids.iter().position(|&id| id == current_id).unwrap_or(0);
        let next_idx = (current_idx + 1) % ids.len();
        let next_id = ids[next_idx];
        // Swap buffer_id and view between the two windows
        let current_buf = self.windows[&current_id].buffer_id;
        let current_view = self.windows[&current_id].view.clone();
        let next_buf = self.windows[&next_id].buffer_id;
        let next_view = self.windows[&next_id].view.clone();
        if let Some(w) = self.windows.get_mut(&current_id) {
            w.buffer_id = next_buf;
            w.view = next_view;
        }
        if let Some(w) = self.windows.get_mut(&next_id) {
            w.buffer_id = current_buf;
            w.view = current_view;
        }
    }

    /// Ctrl-W r/R: rotate windows in the current tab.
    /// `forward=true` rotates downward/rightward, `forward=false` rotates upward/leftward.
    fn rotate_windows(&mut self, forward: bool) {
        let tab = self.active_tab();
        let ids = tab.layout.window_ids();
        if ids.len() < 2 {
            return;
        }
        // Collect (buffer_id, view) for each window in layout order
        let mut data: Vec<_> = ids
            .iter()
            .map(|&id| {
                let w = &self.windows[&id];
                (w.buffer_id, w.view.clone())
            })
            .collect();
        // Rotate the data
        if forward {
            // Last element moves to front
            let last = data.pop().unwrap();
            data.insert(0, last);
        } else {
            // First element moves to back
            let first = data.remove(0);
            data.push(first);
        }
        // Apply rotated data back
        for (i, &id) in ids.iter().enumerate() {
            if let Some(w) = self.windows.get_mut(&id) {
                w.buffer_id = data[i].0;
                w.view = data[i].1.clone();
            }
        }
    }

    /// Jump to end of C-style comment block (]*  or  ]/).
    fn jump_comment_end(&mut self) {
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        for line_idx in start..total {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.contains("*/") {
                self.view_mut().cursor.line = line_idx;
                // Position cursor at the '*' of '*/'
                if let Some(pos) = line.find("*/") {
                    let col = line[..pos].chars().count();
                    self.view_mut().cursor.col = col;
                } else {
                    self.view_mut().cursor.col = 0;
                }
                self.clamp_cursor_col();
                return;
            }
        }
    }

    /// Jump to start of C-style comment block ([*  or  [/).
    fn jump_comment_start(&mut self) {
        let cursor_line = self.view().cursor.line;
        if cursor_line == 0 {
            return;
        }
        for line_idx in (0..cursor_line).rev() {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.contains("/*") {
                self.view_mut().cursor.line = line_idx;
                // Position cursor at the '/' of '/*'
                if let Some(pos) = line.find("/*") {
                    let col = line[..pos].chars().count();
                    self.view_mut().cursor.col = col;
                } else {
                    self.view_mut().cursor.col = 0;
                }
                self.clamp_cursor_col();
                return;
            }
        }
    }

    /// Jump forward to next unmatched `#else` or `#endif` (`]#`).
    /// Uses depth tracking: `#if`/`#ifdef`/`#ifndef` increase depth,
    /// `#endif` decreases depth, `#else`/`#elif` match at depth 0.
    fn jump_preproc_forward(&mut self) {
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        let mut depth: i32 = 0;
        for line_idx in start..total {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if let Some(directive) = Self::preproc_directive(trimmed) {
                match directive {
                    PreprocKind::If => depth += 1,
                    PreprocKind::ElseElif => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                    }
                    PreprocKind::Endif => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                        depth -= 1;
                    }
                }
            }
        }
    }

    /// Jump backward to previous unmatched `#if` or `#else` (`[#`).
    /// Uses depth tracking: `#endif` increases depth,
    /// `#if`/`#ifdef`/`#ifndef` decrease depth, `#else`/`#elif` match at depth 0.
    fn jump_preproc_backward(&mut self) {
        let cursor_line = self.view().cursor.line;
        if cursor_line == 0 {
            return;
        }
        let mut depth: i32 = 0;
        for line_idx in (0..cursor_line).rev() {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if let Some(directive) = Self::preproc_directive(trimmed) {
                match directive {
                    PreprocKind::Endif => depth += 1,
                    PreprocKind::ElseElif => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                    }
                    PreprocKind::If => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                        depth -= 1;
                    }
                }
            }
        }
    }

    /// Classify a trimmed line as a preprocessor directive kind.
    fn preproc_directive(trimmed: &str) -> Option<PreprocKind> {
        if !trimmed.starts_with('#') {
            return None;
        }
        // Strip '#' and optional whitespace after it
        let after_hash = trimmed[1..].trim_start();
        if after_hash.starts_with("ifdef")
            || after_hash.starts_with("ifndef")
            || after_hash.starts_with("if ")
            || after_hash.starts_with("if\t")
            || after_hash == "if"
        {
            Some(PreprocKind::If)
        } else if after_hash.starts_with("else") || after_hash.starts_with("elif") {
            Some(PreprocKind::ElseElif)
        } else if after_hash.starts_with("endif") {
            Some(PreprocKind::Endif)
        } else {
            None
        }
    }

    /// `do` (diff obtain): in a diff view, replace the current line in the active
    /// window with the corresponding line from the other diff window.
    fn diff_obtain(&mut self, changed: &mut bool) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => {
                self.message = "Not in diff mode".to_string();
                return;
            }
        };
        let active = self.active_window_id();
        let other = if active == a_win {
            b_win
        } else if active == b_win {
            a_win
        } else {
            self.message = "Current window is not part of a diff".to_string();
            return;
        };
        let cursor_line = self.view().cursor.line;
        // Get the diff status for the active window
        let diff_status = self
            .diff_results
            .get(&active)
            .and_then(|v| v.get(cursor_line))
            .cloned();
        match diff_status {
            Some(DiffLine::Same) => {
                self.message = "Line is the same in both files".to_string();
            }
            Some(DiffLine::Added) | Some(DiffLine::Removed) | Some(DiffLine::Padding) | None => {
                // Get the corresponding line from the other window
                // For simplicity, use the same line number from the other buffer
                if let Some(other_win) = self.windows.get(&other) {
                    let other_buf_id = other_win.buffer_id;
                    if let Some(other_state) = self.buffer_manager.get(other_buf_id) {
                        if cursor_line < other_state.buffer.len_lines() {
                            let other_line: String = other_state
                                .buffer
                                .content
                                .line(cursor_line)
                                .chars()
                                .collect();
                            // Replace the current line
                            let line_start = self.buffer().line_to_char(cursor_line);
                            let line_end = if cursor_line + 1 < self.buffer().len_lines() {
                                self.buffer().line_to_char(cursor_line + 1)
                            } else {
                                self.buffer().len_chars()
                            };
                            self.start_undo_group();
                            self.delete_with_undo(line_start, line_end);
                            self.insert_with_undo(line_start, &other_line);
                            self.finish_undo_group();
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            *changed = true;
                            self.compute_diff();
                        } else {
                            self.message = "No corresponding line in other file".to_string();
                        }
                    }
                }
            }
        }
    }

    /// `dp` (diff put): in a diff view, replace the corresponding line in the
    /// other diff window with the current line from the active window.
    fn diff_put(&mut self, changed: &mut bool) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => {
                self.message = "Not in diff mode".to_string();
                return;
            }
        };
        let active = self.active_window_id();
        let other = if active == a_win {
            b_win
        } else if active == b_win {
            a_win
        } else {
            self.message = "Current window is not part of a diff".to_string();
            return;
        };
        let cursor_line = self.view().cursor.line;
        // Get the current line from the active buffer
        let current_line: String = self.buffer().content.line(cursor_line).chars().collect();
        // Replace the corresponding line in the other buffer
        if let Some(other_win) = self.windows.get(&other) {
            let other_buf_id = other_win.buffer_id;
            if let Some(other_state) = self.buffer_manager.get_mut(other_buf_id) {
                if cursor_line < other_state.buffer.len_lines() {
                    let line_start = other_state.buffer.line_to_char(cursor_line);
                    let line_end = if cursor_line + 1 < other_state.buffer.len_lines() {
                        other_state.buffer.line_to_char(cursor_line + 1)
                    } else {
                        other_state.buffer.len_chars()
                    };
                    other_state.buffer.delete_range(line_start, line_end);
                    other_state.buffer.insert(line_start, &current_line);
                    other_state.dirty = true;
                    *changed = true;
                    self.compute_diff();
                } else {
                    // Other buffer is shorter — append the line
                    let end = other_state.buffer.len_chars();
                    let needs_newline = end > 0 && other_state.buffer.content.char(end - 1) != '\n';
                    if needs_newline {
                        other_state.buffer.insert(end, "\n");
                    }
                    let end = other_state.buffer.len_chars();
                    other_state.buffer.insert(end, &current_line);
                    other_state.dirty = true;
                    *changed = true;
                    self.compute_diff();
                }
            }
        }
    }

    /// Apply an operator in blockwise mode (rectangle region).
    #[allow(clippy::too_many_arguments)]
    fn apply_blockwise_operator(
        &mut self,
        operator: char,
        start_line: usize,
        end_line: usize,
        left_col: usize,
        right_col: usize,
        changed: &mut bool,
    ) {
        match operator {
            'd' => {
                // Delete the block region
                self.start_undo_group();
                let mut deleted_text = String::new();
                // Process lines in reverse to keep indices stable
                for line_idx in (start_line..=end_line).rev() {
                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let text_len = if line_len > 0
                        && self.buffer().content.char(line_start + line_len - 1) == '\n'
                    {
                        line_len - 1
                    } else {
                        line_len
                    };
                    let from = left_col.min(text_len);
                    let to = (right_col + 1).min(text_len);
                    if from < to {
                        let del: String = self
                            .buffer()
                            .content
                            .slice((line_start + from)..(line_start + to))
                            .chars()
                            .collect();
                        deleted_text = del + "\n" + &deleted_text;
                        self.delete_with_undo(line_start + from, line_start + to);
                    }
                }
                let reg = self.active_register();
                self.set_delete_register(reg, deleted_text, false);
                self.clear_selected_register();
                self.view_mut().cursor.line = start_line;
                self.view_mut().cursor.col = left_col;
                self.clamp_cursor_col();
                self.finish_undo_group();
                *changed = true;
            }
            'y' => {
                // Yank the block region
                let mut yanked = String::new();
                for line_idx in start_line..=end_line {
                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let text_len = if line_len > 0
                        && self.buffer().content.char(line_start + line_len - 1) == '\n'
                    {
                        line_len - 1
                    } else {
                        line_len
                    };
                    let from = left_col.min(text_len);
                    let to = (right_col + 1).min(text_len);
                    if from < to {
                        let chunk: String = self
                            .buffer()
                            .content
                            .slice((line_start + from)..(line_start + to))
                            .chars()
                            .collect();
                        yanked.push_str(&chunk);
                    }
                    yanked.push('\n');
                }
                let reg = self.active_register();
                self.set_yank_register(reg, yanked, false);
                self.clear_selected_register();
            }
            _ => {
                // For other operators (c, ~, u, U, etc.), fall back to charwise
                let start = self.buffer().line_to_char(start_line) + left_col;
                let end_char = self.buffer().line_to_char(end_line) + right_col + 1;
                let max = self.buffer().len_chars();
                self.apply_charwise_operator(operator, start, end_char.min(max), changed);
            }
        }
    }

    /// Try to parse and execute a range filter command like `1,5!sort` or `.!cmd`.
    /// Returns Some(action) if it matched, None otherwise.
    fn try_execute_filter_command(&mut self, cmd: &str) -> Option<EngineAction> {
        // Match patterns: N,M!cmd  or  .!cmd  or  .,.+N!cmd
        // Split on '!' — if there's a range before and a command after, it's a filter.
        let bang_pos = cmd.find('!')?;
        let range_str = &cmd[..bang_pos];
        let filter_cmd = cmd[bang_pos + 1..].trim();
        if filter_cmd.is_empty() || range_str.is_empty() {
            return None;
        }
        // Parse the range. Support: N,M  .,.+N  N  .  %
        let (start_line, end_line) = self.parse_simple_range(range_str)?;
        // Extract the text from the range
        let total_lines = self.buffer().len_lines();
        let start = start_line.min(total_lines.saturating_sub(1));
        let end = end_line.min(total_lines.saturating_sub(1));
        let mut lines_text = String::new();
        for i in start..=end {
            let line: String = self.buffer().content.line(i).chars().collect();
            lines_text.push_str(&line);
        }
        // Pipe through the command
        #[cfg(not(test))]
        let result = {
            use std::io::Write;
            let mut child = match std::process::Command::new("sh")
                .arg("-c")
                .arg(filter_cmd)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    self.message = format!("Filter error: {}", e);
                    return Some(EngineAction::None);
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(lines_text.as_bytes());
            }
            match child.wait_with_output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        self.message = format!("Filter error: {}", stderr.trim());
                        return Some(EngineAction::None);
                    }
                    stdout
                }
                Err(e) => {
                    self.message = format!("Filter error: {}", e);
                    return Some(EngineAction::None);
                }
            }
        };
        #[cfg(test)]
        let result = lines_text.clone(); // No-op in tests

        // Replace the range with the result
        let range_start = self.buffer().line_to_char(start);
        let range_end = if end + 1 < total_lines {
            self.buffer().line_to_char(end + 1)
        } else {
            self.buffer().len_chars()
        };
        self.start_undo_group();
        self.delete_with_undo(range_start, range_end);
        self.insert_with_undo(range_start, &result);
        self.finish_undo_group();
        self.view_mut().cursor.line = start;
        self.view_mut().cursor.col = 0;
        let line_count = result.lines().count();
        self.message = format!("{} lines filtered", line_count);
        Some(EngineAction::None)
    }

    /// Parse a simple line range like "1,5", ".", ".,.+3", "%".
    /// Returns 0-indexed (start_line, end_line).
    fn parse_simple_range(&self, range: &str) -> Option<(usize, usize)> {
        let current_line = self.view().cursor.line;
        let last_line = self.buffer().len_lines().saturating_sub(1);

        if range == "%" {
            return Some((0, last_line));
        }
        if range == "." {
            return Some((current_line, current_line));
        }

        if let Some((left, right)) = range.split_once(',') {
            let start = self.parse_line_addr(left.trim(), current_line, last_line)?;
            let end = self.parse_line_addr(right.trim(), current_line, last_line)?;
            Some((start, end))
        } else {
            let line = self.parse_line_addr(range.trim(), current_line, last_line)?;
            Some((line, line))
        }
    }

    /// Parse a single line address: number (1-indexed), ".", "$", ".+N", ".-N".
    fn parse_line_addr(&self, addr: &str, current: usize, last: usize) -> Option<usize> {
        if addr == "." {
            return Some(current);
        }
        if addr == "$" {
            return Some(last);
        }
        if let Some(offset) = addr.strip_prefix(".+") {
            let n: usize = offset.parse().ok()?;
            return Some((current + n).min(last));
        }
        if let Some(offset) = addr.strip_prefix(".-") {
            let n: usize = offset.parse().ok()?;
            return Some(current.saturating_sub(n));
        }
        // Plain number (1-indexed)
        let n: usize = addr.parse().ok()?;
        if n == 0 {
            return None;
        }
        Some((n - 1).min(last))
    }

    // =======================================================================
    // Mouse selection (called by UI backends after coordinate conversion)
    // =======================================================================

    /// Handle a single mouse click at the given buffer position.
    /// Exits visual mode if active, positions cursor, clears drag state.
    pub fn mouse_click(&mut self, window_id: WindowId, line: usize, col: usize) {
        // Exit visual mode if active
        if matches!(
            self.mode,
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        ) {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
        }
        self.mouse_drag_word_mode = false;
        self.mouse_drag_word_origin = None;
        self.mouse_drag_active = false;
        // Switch to the group that owns this window.
        self.focus_group_for_window(window_id);
        self.set_cursor_for_window(window_id, line, col);
    }

    /// Handle mouse drag to the given buffer position.
    /// On first drag: enters Visual mode with anchor at current cursor.
    /// On subsequent drags: extends selection by moving cursor.
    /// If already in Visual mode (e.g. from double-click word select),
    /// preserves the existing anchor and just extends.
    pub fn mouse_drag(&mut self, window_id: WindowId, line: usize, col: usize) {
        // Ensure this window's group and tab are active.
        self.focus_group_for_window(window_id);
        if self.windows.contains_key(&window_id) {
            self.active_tab_mut().active_window = window_id;
        }

        if !self.mouse_drag_active {
            // First drag event — only set anchor if not already in visual mode
            // (double-click word select already set the anchor at word start).
            // Also require the drag to actually reach a *different* buffer position
            // than the click origin; sub-character mouse jitter on mousedown otherwise
            // silently enters visual mode before `:` can be pressed.
            let cursor = self.view().cursor;
            let moved = line != cursor.line || col != cursor.col;
            if !moved {
                return; // Sub-pixel jitter at same cell — ignore.
            }
            if !matches!(
                self.mode,
                Mode::Visual | Mode::VisualLine | Mode::VisualBlock
            ) {
                self.visual_anchor = Some(cursor);
                self.mode = Mode::Visual;
            }
            self.mouse_drag_active = true;
        }

        // Move cursor to drag position (extends visual selection)
        let buffer = self.buffer();
        let max_line = buffer.content.len_lines().saturating_sub(1);
        let clamped_line = line.min(max_line);
        let max_col = self.get_max_cursor_col(clamped_line);
        let clamped_col = col.min(max_col);

        if self.mouse_drag_word_mode {
            // Word-wise drag: snap to word boundaries
            if let Some((orig_start, orig_end, orig_line)) = self.mouse_drag_word_origin {
                let line_text: Vec<char> =
                    self.buffer().content.line(clamped_line).chars().collect();
                let drag_before_origin = clamped_line < orig_line
                    || (clamped_line == orig_line && clamped_col < orig_start);

                if drag_before_origin {
                    // Dragging before the original word — anchor at word end, cursor at word start
                    let mut word_start = clamped_col.min(line_text.len().saturating_sub(1));
                    if word_start < line_text.len() && Self::is_word_char(line_text[word_start]) {
                        while word_start > 0 && Self::is_word_char(line_text[word_start - 1]) {
                            word_start -= 1;
                        }
                    }
                    self.visual_anchor = Some(Cursor {
                        line: orig_line,
                        col: orig_end,
                    });
                    let view = self.view_mut();
                    view.cursor.line = clamped_line;
                    view.cursor.col = word_start;
                } else {
                    // Dragging after the original word — anchor at word start, cursor at word end
                    let mut word_end = clamped_col.min(line_text.len().saturating_sub(1));
                    if word_end < line_text.len() && Self::is_word_char(line_text[word_end]) {
                        while word_end + 1 < line_text.len()
                            && Self::is_word_char(line_text[word_end + 1])
                        {
                            word_end += 1;
                        }
                    }
                    // Exclude trailing newline
                    if word_end < line_text.len() && line_text[word_end] == '\n' && word_end > 0 {
                        word_end -= 1;
                    }
                    self.visual_anchor = Some(Cursor {
                        line: orig_line,
                        col: orig_start,
                    });
                    let view = self.view_mut();
                    view.cursor.line = clamped_line;
                    view.cursor.col = word_end;
                }
            }
        } else {
            let view = self.view_mut();
            view.cursor.line = clamped_line;
            view.cursor.col = clamped_col;
        }
    }

    /// Handle mouse double-click: select the word under the cursor.
    /// Positions cursor, finds word boundaries, enters Visual mode.
    pub fn mouse_double_click(&mut self, window_id: WindowId, line: usize, col: usize) {
        self.mouse_drag_active = false;
        self.mouse_drag_word_mode = false;
        self.mouse_drag_word_origin = None;
        self.focus_group_for_window(window_id);
        self.set_cursor_for_window(window_id, line, col);

        // Find word boundaries at cursor
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let line_text: Vec<char> = self.buffer().content.line(cursor_line).chars().collect();

        if cursor_col >= line_text.len() || !Self::is_word_char(line_text[cursor_col]) {
            // Clicked on non-word character — don't select
            return;
        }

        // Find word start
        let mut word_start = cursor_col;
        while word_start > 0 && Self::is_word_char(line_text[word_start - 1]) {
            word_start -= 1;
        }

        // Find word end (inclusive)
        let mut word_end = cursor_col;
        while word_end + 1 < line_text.len() && Self::is_word_char(line_text[word_end + 1]) {
            word_end += 1;
        }
        // Exclude trailing newline from word end
        if word_end < line_text.len() && line_text[word_end] == '\n' && word_end > word_start {
            word_end -= 1;
        }

        // Enter visual mode with anchor at word start, cursor at word end
        self.visual_anchor = Some(Cursor {
            line: cursor_line,
            col: word_start,
        });
        let view = self.view_mut();
        view.cursor.col = word_end;
        self.mode = Mode::Visual;
        self.mouse_drag_word_mode = true;
        self.mouse_drag_word_origin = Some((word_start, word_end, cursor_line));
    }

    // =======================================================================
    // Clipboard paste into command/search mode
    // =======================================================================

    /// Paste the first line from the system clipboard into the command buffer.
    /// Works in Command and Search modes. For Search mode with incremental search,
    /// also triggers a search update.
    #[allow(dead_code)]
    pub fn paste_clipboard_to_input(&mut self) {
        let text = match self.clipboard_read {
            Some(ref cb_read) => match cb_read() {
                Ok(t) => t,
                Err(e) => {
                    self.message = format!("Clipboard read failed: {}", e);
                    return;
                }
            },
            None => return,
        };
        self.paste_text_to_input(&text);
    }

    /// Paste the given text into the command/search buffer (first line only).
    /// Called by backends that have already fetched the clipboard text themselves.
    pub fn paste_text_to_input(&mut self, text: &str) {
        let first_line = text.lines().next().unwrap_or("");
        if first_line.is_empty() {
            return;
        }
        match self.mode {
            Mode::Command | Mode::Search => {
                self.command_insert_str(first_line);
                if self.mode == Mode::Search && self.settings.incremental_search {
                    self.perform_incremental_search();
                }
            }
            _ => {}
        }
    }

    /// Pre-load clipboard text into the `+` and `*` registers.
    /// Called by GTK backend after an async GDK clipboard read, before paste.
    pub fn load_clipboard_register(&mut self, text: String) {
        self.registers.insert('+', (text.clone(), false));
        self.registers.insert('*', (text, false));
    }

    /// Pre-load clipboard text into `"`, `+`, and `*` before a p/P keypress.
    ///
    /// If the clipboard content exactly matches what is already in `"`, the
    /// existing `is_linewise` flag is **preserved** — this covers the common
    /// `yy` → `p` flow where the yank wrote linewise text to both the register
    /// and the system clipboard.  When the clipboard holds different text (from
    /// another application) `is_linewise` is set to `false` as usual.
    pub fn load_clipboard_for_paste(&mut self, text: String) {
        let existing_lw = self
            .registers
            .get(&'"')
            .map(|(c, lw)| c == &text && *lw)
            .unwrap_or(false);
        self.registers.insert('"', (text.clone(), existing_lw));
        self.registers.insert('+', (text.clone(), false));
        self.registers.insert('*', (text, false));
    }

    // =======================================================================
    // LSP integration
    // =======================================================================

    /// Ensure the LSP manager is initialized (lazy — created on first use).
    fn ensure_lsp_manager(&mut self) {
        if !self.settings.lsp_enabled || self.lsp_manager.is_some() {
            return;
        }
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut mgr = LspManager::new(root, &self.settings.lsp_servers);
        mgr.set_ext_manifests(
            self.ext_installed_manifests(),
            self.ext_available_manifests(),
        );
        self.lsp_manager = Some(mgr);
    }

    /// Ensure LSP is started for the active buffer (lazy — called on tab switch).
    /// This is idempotent: if the server is already running, didOpen is a no-op.
    pub fn lsp_ensure_active_buffer(&mut self) {
        let bid = self.active_buffer_id();
        let has_file = self
            .buffer_manager
            .get(bid)
            .and_then(|s| s.file_path.as_ref())
            .is_some();
        if has_file {
            self.lsp_did_open(bid);
        }
    }

    /// Notify LSP that a file was opened.
    fn lsp_did_open(&mut self, buffer_id: BufferId) {
        // Fire plugin "open" hook regardless of LSP enabled state
        if let Some(state) = self.buffer_manager.get(buffer_id) {
            if let Some(path) = state.file_path.clone() {
                let path_str = path.to_string_lossy().into_owned();
                self.plugin_event("open", &path_str);
                self.plugin_event("BufNew", &path_str);
                self.plugin_event("BufEnter", &path_str);
            }
        }
        // Fire cursor_move so position-aware plugins (e.g. git-insights blame) annotate
        // the initial cursor line immediately on file open without requiring a keypress.
        self.fire_cursor_move_hook_now();
        if !self.settings.lsp_enabled {
            return;
        }
        let (path, text, lang_id) = {
            let state = match self.buffer_manager.get(buffer_id) {
                Some(s) => s,
                None => return,
            };
            let path = match &state.file_path {
                Some(p) => p.clone(),
                None => return,
            };
            // User language_map takes priority; fall back to built-in extension table
            let lang_id = lsp::language_id_from_path_with_map(&path, &self.settings.language_map)
                .or_else(|| state.lsp_language_id.clone());
            let lang_id = match lang_id {
                Some(l) => l,
                None => return,
            };
            (path, state.buffer.to_string(), lang_id)
        };
        self.ensure_lsp_manager();
        let no_server = if let Some(mgr) = &mut self.lsp_manager {
            mgr.notify_did_open(&path, &text).err()
        } else {
            None
        };
        // Request semantic tokens after opening a file.
        self.lsp_request_semantic_tokens(&path);
        // Show extension hint based on VimCode extension state (independent of LSP binary
        // availability — ext_remove intentionally leaves the binary on disk).
        let manifests = self.ext_available_manifests();
        if let Some(manifest) = extensions::find_manifest_for_language_id(&manifests, &lang_id) {
            let name = &manifest.name;
            if !self.extension_state.is_installed(name)
                && !self.extension_state.is_dismissed(name)
                && !self.prompted_extensions.contains(name.as_str())
            {
                self.prompted_extensions.insert(name.to_string());
                self.ext_hint_pending_name = Some(name.to_string());
                self.message = format!(
                    "No {} extension — :ExtInstall {}  (N to dismiss)",
                    manifest.display_name, name
                );
            }
        } else if let Some(err) = no_server {
            // Show dependency errors prominently; generic "no server" only as fallback.
            self.message = err;
        }
    }

    // ── Extension registry + sidebar ──────────────────────────────────────────

    /// Return the list of available extensions from the cached registry.
    /// Return manifests only for extensions that are installed.
    /// Used for LSP manager — only start servers when the extension is installed.
    pub fn ext_installed_manifests(&self) -> Vec<extensions::ExtensionManifest> {
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| self.extension_state.is_installed(&m.name))
            .collect()
    }

    pub fn ext_available_manifests(&self) -> Vec<extensions::ExtensionManifest> {
        let mut result: Vec<extensions::ExtensionManifest> =
            self.ext_registry.clone().unwrap_or_default();

        // Merge local extensions: scan extensions/*/manifest.toml in config dir
        // so developers can test extensions locally before publishing to the registry.
        let ext_base = paths::vimcode_config_dir().join("extensions");
        if let Ok(entries) = std::fs::read_dir(&ext_base) {
            for entry in entries.filter_map(|e| e.ok()) {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let manifest_path = dir.join("manifest.toml");
                if let Ok(toml_str) = std::fs::read_to_string(&manifest_path) {
                    if let Some(manifest) = extensions::ExtensionManifest::parse(&toml_str) {
                        // Local manifest overrides registry entry with same name
                        result.retain(|m| !m.name.eq_ignore_ascii_case(&manifest.name));
                        result.push(manifest);
                    }
                }
            }
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Spawn a background thread to fetch all configured extension registries.
    /// Result arrives via `ext_registry_rx`.
    pub fn ext_refresh(&mut self) {
        if self.ext_registry_fetching {
            return; // already in progress
        }
        let urls = self.settings.extension_registries.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut merged: Vec<extensions::ExtensionManifest> = Vec::new();
            for url in &urls {
                if let Some(mut entries) = registry::fetch_registry(url) {
                    let base = registry::base_url_from_registry(url);
                    for m in &mut entries {
                        m.registry_base_url = base.clone();
                    }
                    // Later registries override earlier ones on name collision
                    for entry in entries {
                        merged.retain(|m| !m.name.eq_ignore_ascii_case(&entry.name));
                        merged.push(entry);
                    }
                }
            }
            let result = if merged.is_empty() && !urls.is_empty() {
                None // all fetches failed
            } else {
                Some(merged)
            };
            let _ = tx.send(result);
        });
        self.ext_registry_rx = Some(rx);
        self.ext_registry_fetching = true;
        self.message = "Fetching extension registries...".to_string();
    }

    /// Non-blocking check for a completed registry fetch.
    /// Call this from `handle_key` / `poll_lsp`.
    pub fn poll_ext_registry(&mut self) -> bool {
        let result = if let Some(rx) = &self.ext_registry_rx {
            rx.try_recv().ok()
        } else {
            return false;
        };
        if let Some(maybe_reg) = result {
            self.ext_registry_fetching = false;
            self.ext_registry_rx = None;
            match maybe_reg {
                Some(entries) => {
                    let count = entries.len();
                    registry::save_cache(&entries);
                    self.ext_registry = Some(entries);
                    self.message = format!("Extension registry updated ({count} extensions)");
                }
                None => {
                    self.message = "Registry fetch failed — try again later".to_string();
                }
            }
            true
        } else {
            false
        }
    }

    /// Resolve the base URL for downloading extension files.
    /// Uses the manifest's `registry_base_url` if available, otherwise derives it
    /// from the first configured registry URL (the field is `#[serde(skip)]` so it's
    /// empty when loaded from cache).
    fn resolve_registry_base_url(&self, manifest: &extensions::ExtensionManifest) -> String {
        if !manifest.registry_base_url.is_empty() {
            return manifest.registry_base_url.clone();
        }
        self.settings
            .extension_registries
            .first()
            .map(|url| registry::base_url_from_registry(url))
            .unwrap_or_default()
    }

    /// Install an extension by name: download scripts, run LSP/DAP install, mark installed.
    pub fn ext_install_from_registry(&mut self, name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));
        let manifest = match manifest {
            Some(m) => m,
            None => {
                self.message =
                    format!("Unknown extension '{name}' — try :ExtRefresh then :ExtList");
                return;
            }
        };
        let ext_name = manifest.name.clone();

        // Download scripts from the registry (skip files already on disk for local dev)
        let ext_dir = paths::vimcode_config_dir()
            .join("extensions")
            .join(&ext_name);
        let base_url = self.resolve_registry_base_url(&manifest);
        if !manifest.scripts.is_empty()
            && !base_url.is_empty()
            && std::fs::create_dir_all(&ext_dir).is_ok()
        {
            for script in &manifest.scripts {
                let dest = ext_dir.join(script);
                if !dest.exists() {
                    let url = format!("{}/{}/{}", base_url, ext_name, script);
                    let _ = registry::download_script(&url, &dest);
                }
            }
        }

        let mut status_parts: Vec<String> = Vec::new();
        let mut install_commands: Vec<String> = Vec::new();

        // ── LSP ──────────────────────────────────────────────────────────────
        // Check if any LSP binary is already on PATH (idempotent: skip install
        // if the server is already available, e.g. via `rustup component add`).
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            let found_bin = all_lsp.iter().copied().find(|b| binary_on_path(b));
            if let Some(bin) = found_bin {
                status_parts.push(format!("LSP: {bin} ✓"));
            } else if !manifest.lsp.install_cmd_for_platform().is_empty() {
                let lsp_key = format!("ext:{ext_name}:lsp");
                self.lsp_installing.insert(lsp_key.clone());
                install_commands.push(manifest.lsp.install_cmd_for_platform().to_string());
                self.pending_install_context = Some(InstallContext {
                    ext_name: ext_name.clone(),
                    install_key: lsp_key,
                });
                status_parts.push(format!("LSP: installing {}…", manifest.lsp.binary));
            }
        }

        // ── DAP ──────────────────────────────────────────────────────────────
        // Check PATH first (idempotent).  Only auto-install if the manifest
        // provides an explicit dap.install command.  An empty dap.install means
        // "this adapter needs a manual/complex install" — guide the user to run
        // :DapInstall instead of silently attempting a potentially large download.
        if !manifest.dap.adapter.is_empty() {
            let dap_binary = manifest.dap.binary.as_str();
            let already_on_path = !dap_binary.is_empty() && binary_on_path(dap_binary);
            if already_on_path {
                status_parts.push(format!("DAP: {dap_binary} ✓"));
            } else if !manifest.dap.install_cmd_for_platform().is_empty() {
                let dap_key = format!("dap:{}", manifest.dap.adapter);
                self.lsp_installing.insert(dap_key.clone());
                install_commands.push(manifest.dap.install_cmd_for_platform().to_string());
                // Only set install context if LSP didn't already set it.
                if self.pending_install_context.is_none() {
                    self.pending_install_context = Some(InstallContext {
                        ext_name: ext_name.clone(),
                        install_key: dap_key,
                    });
                }
                status_parts.push(format!("DAP: installing {}…", manifest.dap.adapter));
            } else if !dap_binary.is_empty() {
                // No auto-install — guide the user to :DapInstall.
                status_parts.push(format!(
                    "DAP: run :DapInstall {ext_name} to set up {dap_binary}"
                ));
            }
        }

        // If there are install commands, combine them and store for the UI to run
        // in a visible terminal pane.
        if !install_commands.is_empty() {
            let combined = install_commands.join(" && ");
            let header = format!("echo '── Installing {ext_name} ──'");
            self.pending_terminal_command = Some(format!("{header} && {combined}"));
        }

        // Mark installed with version and persist
        self.extension_state
            .mark_installed_version(&ext_name, &manifest.version);
        let _ = self.extension_state.save();

        // Reload plugins so newly extracted scripts are active
        self.plugin_manager = None;
        self.plugin_init();

        // Kick-start LSP for the current buffer if it matches this extension's languages.
        // Without this, the user would have to re-open the file to get LSP support.
        let active_bid = self.active_buffer_id();
        if let Some(state) = self.buffer_manager.get(active_bid) {
            let buf_lang = state.lsp_language_id.clone().or_else(|| {
                state
                    .file_path
                    .as_ref()
                    .and_then(|p| lsp::language_id_from_path(p))
            });
            let matches = buf_lang
                .as_ref()
                .is_some_and(|lang| manifest.language_ids.iter().any(|l| l == lang));
            if matches {
                self.lsp_did_open(active_bid);
            }
        }

        self.message = if status_parts.is_empty() {
            format!("Extension '{ext_name}' installed")
        } else {
            format!(
                "Extension '{ext_name}' installed — {}",
                status_parts.join(", ")
            )
        };
    }

    /// Open the README for the currently selected extension in the sidebar.
    /// Used by Enter and double-click.
    pub fn ext_open_selected_readme(&mut self) {
        let manifests = self.ext_available_manifests();
        let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
        let manifest = if in_installed {
            let installed = self.ext_installed_items();
            installed
                .get(idx)
                .and_then(|m| manifests.iter().find(|r| r.name == m.name))
        } else {
            let available = self.ext_available_items();
            available
                .get(idx)
                .and_then(|a| manifests.iter().find(|m| m.name == a.name))
        };
        if let Some(manifest) = manifest {
            let name = manifest.name.clone();
            let display = if manifest.display_name.is_empty() {
                name.clone()
            } else {
                manifest.display_name.clone()
            };
            let base_url = self.resolve_registry_base_url(manifest);
            let readme_path = paths::vimcode_config_dir()
                .join("extensions")
                .join(&name)
                .join("README.md");
            let content = std::fs::read_to_string(&readme_path)
                .ok()
                .or_else(|| registry::fetch_readme(&base_url, &name));
            if let Some(content) = content {
                self.open_markdown_preview_in_tab(&content, &display);
            } else {
                self.message = format!("No README available for '{name}'. Press i to install.");
            }
        }
    }

    /// Show a confirmation dialog before removing an extension.
    /// Lists the tools that would be removed and offers three choices.
    fn ext_show_remove_dialog(&mut self, name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));

        // Collect tool binary names that are currently installed on PATH.
        let mut tool_names: Vec<String> = Vec::new();
        if let Some(ref m) = manifest {
            if !m.lsp.binary.is_empty() && binary_on_path(&m.lsp.binary) {
                tool_names.push(m.lsp.binary.clone());
            }
            if !m.dap.binary.is_empty() && binary_on_path(&m.dap.binary) {
                // Avoid duplicates (some extensions share a binary).
                if !tool_names.contains(&m.dap.binary) {
                    tool_names.push(m.dap.binary.clone());
                }
            }
        }

        let mut body = vec![format!("Remove extension '{name}'?")];
        if tool_names.is_empty() {
            body.push("This will remove extension scripts and settings.".to_string());
        } else {
            body.push(String::new());
            body.push(format!("Installed tools: {}", tool_names.join(", ")));
        }

        let buttons = if tool_names.is_empty() {
            vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: 'c',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "Remove".into(),
                    hotkey: 'r',
                    action: "remove".into(),
                },
            ]
        } else {
            vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: 'c',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "Keep Tools".into(),
                    hotkey: 'k',
                    action: "keep_tools".into(),
                },
                DialogButton {
                    label: "Remove All".into(),
                    hotkey: 'a',
                    action: "remove_all".into(),
                },
            ]
        };

        self.pending_ext_remove = Some(name.to_string());
        self.show_dialog("ext_remove", "Remove Extension", body, buttons);
    }

    /// Remove an extension: unmark as installed, delete its Lua scripts.
    /// When `remove_tools` is true, also delete LSP/DAP binaries from PATH.
    pub fn ext_remove(&mut self, name: &str, remove_tools: bool) {
        let name = name.to_string();

        // Optionally remove installed tool binaries before clearing state.
        if remove_tools {
            self.ext_remove_tools(&name);
        }

        self.extension_state.installed.retain(|e| e.name != name);
        let _ = self.extension_state.save();

        // Remove in-memory extension settings
        self.ext_settings.remove(&name);
        self.ext_settings_collapsed.remove(&name);

        let ext_dir = paths::vimcode_config_dir().join("extensions").join(&name);
        let _ = std::fs::remove_dir_all(&ext_dir);

        // Reload plugins so removed scripts are no longer active
        self.plugin_manager = None;
        self.plugin_init();

        if remove_tools {
            self.message = format!("Extension '{name}' and its tools removed");
        } else {
            self.message = format!("Extension '{name}' removed (tools kept on PATH)");
        }

        // Keep sidebar selection in bounds.
        if self.ext_flat_item_count() == 0 {
            self.ext_sidebar_sections_expanded[1] = true;
        }
        let new_total = self.ext_flat_item_count();
        if new_total > 0 && self.ext_sidebar_selected >= new_total {
            self.ext_sidebar_selected = new_total - 1;
        }
    }

    /// Remove LSP/DAP tool binaries installed by an extension.
    /// Only removes binaries found under well-known managed directories
    /// (~/.local/bin, ~/.local/share/<name>, Mason bin dir).
    fn ext_remove_tools(&mut self, name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));
        let manifest = match manifest {
            Some(m) => m,
            None => return,
        };

        let mut removed: Vec<String> = Vec::new();

        // Collect all binary names to check.
        let mut bins: Vec<String> = Vec::new();
        if !manifest.lsp.binary.is_empty() {
            bins.push(manifest.lsp.binary.clone());
        }
        if !manifest.dap.binary.is_empty() && !bins.contains(&manifest.dap.binary) {
            bins.push(manifest.dap.binary.clone());
        }

        // Safe directories where we allow automatic removal.
        let home = std::env::var("HOME").unwrap_or_default();
        let mut safe_dirs: Vec<PathBuf> = vec![
            PathBuf::from(&home).join(".local/bin"),
            PathBuf::from(&home).join(".cargo/bin"),
        ];
        // Also check Mason's bin dir if it exists.
        let mason_dir = PathBuf::from(&home).join(".local/share/nvim/mason/bin");
        if mason_dir.is_dir() {
            safe_dirs.push(mason_dir);
        }

        for bin_name in &bins {
            // Remove binary from safe dirs.
            for dir in &safe_dirs {
                let path = dir.join(bin_name);
                if path.exists() && std::fs::remove_file(&path).is_ok() {
                    removed.push(format!("{}", path.display()));
                }
            }
            // Remove associated data dir (e.g. ~/.local/share/lua-language-server/).
            let data_dir = PathBuf::from(&home).join(".local/share").join(bin_name);
            if data_dir.is_dir() {
                let _ = std::fs::remove_dir_all(&data_dir);
            }
        }

        if !removed.is_empty() {
            super::lsp_manager::install_log(&format!(
                "[ext-remove] Removed tools for '{name}': {}",
                removed.join(", ")
            ));
        }
    }

    /// Update a single extension: re-download scripts and update version.
    pub fn ext_update_one(&mut self, name: &str) {
        if !self.extension_state.is_installed(name) {
            self.message = format!("Extension '{name}' is not installed");
            return;
        }
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name.eq_ignore_ascii_case(name));
        let manifest = match manifest {
            Some(m) => m,
            None => {
                self.message = format!("Extension '{name}' not found in registry");
                return;
            }
        };

        let ext_name = manifest.name.clone();
        let new_version = manifest.version.clone();

        // Re-download scripts (overwrite existing files)
        let ext_dir = paths::vimcode_config_dir()
            .join("extensions")
            .join(&ext_name);
        let base_url = self.resolve_registry_base_url(&manifest);
        if !manifest.scripts.is_empty()
            && !base_url.is_empty()
            && std::fs::create_dir_all(&ext_dir).is_ok()
        {
            for script in &manifest.scripts {
                let dest = ext_dir.join(script);
                let url = format!("{}/{}/{}", base_url, ext_name, script);
                let _ = registry::download_script(&url, &dest);
            }
        }

        // Check if LSP/DAP install commands need to run (only if binaries missing)
        let mut install_commands: Vec<String> = Vec::new();
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            if all_lsp.iter().copied().all(|b| !binary_on_path(b)) {
                let cmd = manifest.lsp.install_cmd_for_platform();
                if !cmd.is_empty() {
                    install_commands.push(cmd.to_string());
                }
            }
        }
        if !manifest.dap.adapter.is_empty()
            && !manifest.dap.binary.is_empty()
            && !binary_on_path(&manifest.dap.binary)
        {
            let cmd = manifest.dap.install_cmd_for_platform();
            if !cmd.is_empty() {
                install_commands.push(cmd.to_string());
            }
        }

        if !install_commands.is_empty() {
            let combined = install_commands.join(" && ");
            let header = format!("echo '── Updating {ext_name} ──'");
            self.pending_terminal_command = Some(format!("{header} && {combined}"));
        }

        // Update version
        self.extension_state
            .mark_installed_version(&ext_name, &new_version);
        let _ = self.extension_state.save();

        // Reload plugins
        self.plugin_manager = None;
        self.plugin_init();

        self.message = if new_version.is_empty() {
            format!("Extension '{ext_name}' updated")
        } else {
            format!("Extension '{ext_name}' updated to v{new_version}")
        };
    }

    /// Update all installed extensions that have newer versions available.
    pub fn ext_update_all(&mut self) {
        let manifests = self.ext_available_manifests();
        let mut updated = Vec::new();
        for manifest in &manifests {
            let installed_ver = self.extension_state.installed_version(&manifest.name);
            if installed_ver.is_empty() && self.extension_state.is_installed(&manifest.name) {
                // No version tracked — always update
                updated.push(manifest.name.clone());
            } else if self.extension_state.is_installed(&manifest.name)
                && !manifest.version.is_empty()
                && manifest.version != installed_ver
            {
                updated.push(manifest.name.clone());
            }
        }
        if updated.is_empty() {
            self.message = "All extensions are up to date".to_string();
            return;
        }
        let count = updated.len();
        for name in &updated {
            // Re-download scripts for each
            if let Some(manifest) = manifests.iter().find(|m| &m.name == name) {
                let ext_dir = paths::vimcode_config_dir().join("extensions").join(name);
                let base_url = self.resolve_registry_base_url(manifest);
                if !manifest.scripts.is_empty()
                    && !base_url.is_empty()
                    && std::fs::create_dir_all(&ext_dir).is_ok()
                {
                    for script in &manifest.scripts {
                        let dest = ext_dir.join(script);
                        let url = format!("{}/{}/{}", base_url, name, script);
                        let _ = registry::download_script(&url, &dest);
                    }
                }
                self.extension_state
                    .mark_installed_version(name, &manifest.version);
            }
        }
        let _ = self.extension_state.save();
        self.plugin_manager = None;
        self.plugin_init();
        self.message = format!("{count} extension(s) updated: {}", updated.join(", "));
    }

    /// Returns true if a newer version is available for the given extension.
    pub fn ext_has_update(&self, name: &str) -> bool {
        if !self.extension_state.is_installed(name) {
            return false;
        }
        let installed_ver = self.extension_state.installed_version(name);
        if let Some(registry) = &self.ext_registry {
            if let Some(manifest) = registry.iter().find(|m| m.name == name) {
                if manifest.version.is_empty() {
                    return false;
                }
                return installed_ver.is_empty() || manifest.version != installed_ver;
            }
        }
        false
    }

    // ─── Extension Panel helpers ────────────────────────────────────────────

    /// Compute the total number of flat items across all sections of the active extension panel.
    /// Check if a tree item is visible (all ancestors expanded).
    fn ext_panel_item_visible(
        &self,
        panel_name: &str,
        item: &plugin::ExtPanelItem,
        items: &[plugin::ExtPanelItem],
    ) -> bool {
        if item.parent_id.is_empty() {
            return true;
        }
        // Walk up the parent chain
        let mut pid = &item.parent_id;
        loop {
            if pid.is_empty() {
                return true;
            }
            // Find the parent item
            if let Some(parent) = items.iter().find(|i| i.id == *pid) {
                let is_expanded = self
                    .ext_panel_tree_expanded
                    .get(&(panel_name.to_string(), parent.id.clone()))
                    .copied()
                    .unwrap_or(parent.expanded);
                if !is_expanded {
                    return false;
                }
                pid = &parent.parent_id;
            } else {
                return true; // parent not found, show the item
            }
        }
    }

    /// Count visible items in a section (accounting for collapsed tree nodes).
    fn ext_panel_visible_count(&self, panel_name: &str, items: &[plugin::ExtPanelItem]) -> usize {
        items
            .iter()
            .filter(|item| self.ext_panel_item_visible(panel_name, item, items))
            .count()
    }

    /// Return the indices of visible items in a section.
    pub fn ext_panel_visible_indices(
        &self,
        panel_name: &str,
        items: &[plugin::ExtPanelItem],
    ) -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.ext_panel_item_visible(panel_name, item, items))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn ext_panel_flat_len(&self) -> usize {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => return 0,
        };
        let reg = match self.ext_panels.get(&panel_name) {
            Some(r) => r,
            None => return 0,
        };
        let expanded = self.ext_panel_sections_expanded.get(&panel_name);
        let mut count = 0;
        for (si, section) in reg.sections.iter().enumerate() {
            count += 1; // section header
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.clone(), section.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    count += self.ext_panel_visible_count(&panel_name, items);
                }
            }
        }
        count
    }

    /// Given a flat index, return (section_index, item_index_within_section).
    /// If the flat index lands on a section header, item_index is `usize::MAX`.
    /// item_index refers to the original (unfiltered) index in the items Vec.
    pub fn ext_panel_flat_to_section(&self, flat: usize) -> Option<(usize, usize)> {
        let panel_name = self.ext_panel_active.clone()?;
        let reg = self.ext_panels.get(&panel_name)?;
        let expanded = self.ext_panel_sections_expanded.get(&panel_name);
        let mut pos = 0;
        for (si, section) in reg.sections.iter().enumerate() {
            if pos == flat {
                return Some((si, usize::MAX));
            }
            pos += 1;
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.clone(), section.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    let visible = self.ext_panel_visible_indices(&panel_name, items);
                    if flat < pos + visible.len() {
                        return Some((si, visible[flat - pos]));
                    }
                    pos += visible.len();
                }
            }
        }
        None
    }

    /// Find the flat index of an item by its ID within a specific section.
    /// Returns `None` if the panel, section, or item is not found.
    pub fn ext_panel_find_flat_index(
        &self,
        panel_name: &str,
        section_name: &str,
        item_id: &str,
    ) -> Option<usize> {
        let reg = self.ext_panels.get(panel_name)?;
        let expanded = self.ext_panel_sections_expanded.get(panel_name);
        let mut pos = 0;
        for (si, section) in reg.sections.iter().enumerate() {
            pos += 1; // section header
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.to_string(), section.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    let visible = self.ext_panel_visible_indices(panel_name, items);
                    if section == section_name {
                        for &vi in &visible {
                            if items[vi].id == item_id
                                || items[vi].id.starts_with(item_id)
                                || item_id.starts_with(&items[vi].id)
                            {
                                return Some(pos + visible.iter().position(|&x| x == vi).unwrap());
                            }
                        }
                    }
                    pos += visible.len();
                }
            } else if section == section_name {
                // Section is collapsed — can't find the item
                return None;
            }
        }
        None
    }

    /// Programmatically reveal an item in an extension panel: expand its section,
    /// set the selection to point at it, and adjust scroll.
    pub fn ext_panel_reveal_item(&mut self, panel_name: &str, section_name: &str, item_id: &str) {
        // Ensure the target section is expanded
        if let Some(reg) = self.ext_panels.get(panel_name) {
            if let Some(si) = reg.sections.iter().position(|s| s == section_name) {
                let expanded = self
                    .ext_panel_sections_expanded
                    .entry(panel_name.to_string())
                    .or_insert_with(|| vec![true; reg.sections.len()]);
                if let Some(v) = expanded.get_mut(si) {
                    *v = true;
                }
            }
        }
        // Find the flat index and set selection
        if let Some(flat_idx) = self.ext_panel_find_flat_index(panel_name, section_name, item_id) {
            self.ext_panel_selected = flat_idx;
            // Center the item in the viewport
            self.ext_panel_scroll_top = flat_idx.saturating_sub(5);
        }
    }

    /// Ensure the selected ext panel item is visible by adjusting scroll.
    /// `visible_rows` is the approximate number of rows visible in the panel viewport.
    fn ext_panel_ensure_visible(&mut self, visible_rows: usize) {
        let rows = if visible_rows == 0 { 20 } else { visible_rows };
        if self.ext_panel_selected < self.ext_panel_scroll_top {
            self.ext_panel_scroll_top = self.ext_panel_selected;
        } else if self.ext_panel_selected >= self.ext_panel_scroll_top + rows {
            self.ext_panel_scroll_top = self.ext_panel_selected.saturating_sub(rows - 1);
        }
    }

    /// Handle keyboard input for an extension panel.
    /// Returns `true` if the key was consumed.
    pub fn handle_ext_panel_key(&mut self, key: &str, _ctrl: bool, _unicode: Option<char>) -> bool {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => {
                self.ext_panel_has_focus = false;
                return true;
            }
        };

        // Any key closes help popup
        if self.ext_panel_help_open {
            self.ext_panel_help_open = false;
            return true;
        }

        match key {
            "q" | "Escape" => {
                self.ext_panel_has_focus = false;
            }
            "j" | "Down" => {
                let max = self.ext_panel_flat_len();
                if max > 0 && self.ext_panel_selected + 1 < max {
                    self.ext_panel_selected += 1;
                }
                self.ext_panel_ensure_visible(0);
            }
            "k" | "Up" => {
                if self.ext_panel_selected > 0 {
                    self.ext_panel_selected -= 1;
                }
                self.ext_panel_ensure_visible(0);
            }
            "g" => {
                self.ext_panel_selected = 0;
                self.ext_panel_scroll_top = 0;
            }
            "G" => {
                let max = self.ext_panel_flat_len();
                if max > 0 {
                    self.ext_panel_selected = max - 1;
                }
                self.ext_panel_ensure_visible(0);
            }
            "/" => {
                // Activate the input field for filtering/searching within the panel.
                self.ext_panel_input_active = true;
            }
            "Tab" => {
                // Toggle expand/collapse — works on section headers AND expandable tree items
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    if item_idx == usize::MAX {
                        // Section header: toggle section expand
                        let expanded = self
                            .ext_panel_sections_expanded
                            .entry(panel_name.clone())
                            .or_default();
                        while expanded.len() <= si {
                            expanded.push(true);
                        }
                        expanded[si] = !expanded[si];
                    } else {
                        // Item: toggle tree node expand if expandable
                        let reg = self.ext_panels.get(&panel_name).cloned();
                        if let Some(reg) = reg {
                            if let Some(section) = reg.sections.get(si) {
                                let key = (panel_name.clone(), section.clone());
                                let is_expandable = self
                                    .ext_panel_items
                                    .get(&key)
                                    .and_then(|items| items.get(item_idx))
                                    .map(|item| item.expandable)
                                    .unwrap_or(false);
                                if is_expandable {
                                    let item_id = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.id.clone())
                                        .unwrap_or_default();
                                    let default_expanded = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.expanded)
                                        .unwrap_or(false);
                                    let tree_key = (panel_name.clone(), item_id.clone());
                                    let currently = self
                                        .ext_panel_tree_expanded
                                        .get(&tree_key)
                                        .copied()
                                        .unwrap_or(default_expanded);
                                    self.ext_panel_tree_expanded.insert(tree_key, !currently);
                                    // Fire expand/collapse event
                                    let event = if currently {
                                        "panel_collapse"
                                    } else {
                                        "panel_expand"
                                    };
                                    let arg = format!(
                                        "{}|{}|{}||{}",
                                        panel_name, section, item_id, self.ext_panel_selected
                                    );
                                    self.plugin_event(event, &arg);
                                }
                            }
                        }
                    }
                }
            }
            "Return" => {
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    if item_idx == usize::MAX {
                        // Section header: toggle section expand
                        let expanded = self
                            .ext_panel_sections_expanded
                            .entry(panel_name.clone())
                            .or_default();
                        while expanded.len() <= si {
                            expanded.push(true);
                        }
                        expanded[si] = !expanded[si];
                    } else {
                        // Check if item is expandable — if so, toggle expand
                        let reg = self.ext_panels.get(&panel_name).cloned();
                        let mut toggled = false;
                        if let Some(ref reg) = reg {
                            if let Some(section) = reg.sections.get(si) {
                                let key = (panel_name.clone(), section.clone());
                                let is_expandable = self
                                    .ext_panel_items
                                    .get(&key)
                                    .and_then(|items| items.get(item_idx))
                                    .map(|item| item.expandable)
                                    .unwrap_or(false);
                                if is_expandable {
                                    let item_id = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.id.clone())
                                        .unwrap_or_default();
                                    let default_expanded = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.expanded)
                                        .unwrap_or(false);
                                    let tree_key = (panel_name.clone(), item_id.clone());
                                    let currently = self
                                        .ext_panel_tree_expanded
                                        .get(&tree_key)
                                        .copied()
                                        .unwrap_or(default_expanded);
                                    self.ext_panel_tree_expanded.insert(tree_key, !currently);
                                    let event = if currently {
                                        "panel_collapse"
                                    } else {
                                        "panel_expand"
                                    };
                                    let arg = format!(
                                        "{}|{}|{}||{}",
                                        panel_name, section, item_id, self.ext_panel_selected
                                    );
                                    self.plugin_event(event, &arg);
                                    toggled = true;
                                }
                            }
                        }
                        // If not expandable, fire panel_select
                        if !toggled {
                            if let Some(reg) = reg {
                                if let Some(section) = reg.sections.get(si) {
                                    let key = (panel_name.clone(), section.clone());
                                    let id = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.id.clone())
                                        .unwrap_or_default();
                                    let arg =
                                        format!("{}|{}|{}||{}", panel_name, section, id, item_idx);
                                    self.plugin_event("panel_select", &arg);
                                }
                            }
                        }
                    }
                }
            }
            "?" => {
                if self.ext_panel_help_bindings.contains_key(&panel_name) {
                    self.ext_panel_help_open = true;
                }
            }
            other => {
                // Check if the key matches an action button on the selected item
                let mut action_label = None;
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    if item_idx != usize::MAX {
                        let reg = self.ext_panels.get(&panel_name).cloned();
                        if let Some(reg) = &reg {
                            if let Some(section) = reg.sections.get(si) {
                                let key = (panel_name.clone(), section.clone());
                                if let Some(items) = self.ext_panel_items.get(&key) {
                                    if let Some(item) = items.get(item_idx) {
                                        for action in &item.actions {
                                            if action.key == other {
                                                action_label = Some(action.label.clone());
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Fire panel_action event
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    let reg = self.ext_panels.get(&panel_name).cloned();
                    if let Some(reg) = reg {
                        if let Some(section) = reg.sections.get(si) {
                            let key = (panel_name.clone(), section.clone());
                            let id = if item_idx != usize::MAX {
                                self.ext_panel_items
                                    .get(&key)
                                    .and_then(|items| items.get(item_idx))
                                    .map(|item| item.id.clone())
                                    .unwrap_or_default()
                            } else {
                                String::new()
                            };
                            // Use action label as key if matched, otherwise original key
                            let event_key = action_label.as_deref().unwrap_or(other);
                            let arg = format!(
                                "{}|{}|{}|{}|{}",
                                panel_name, section, id, event_key, self.ext_panel_selected
                            );
                            self.plugin_event("panel_action", &arg);
                        }
                    }
                }
            }
        }
        true
    }

    /// Handle double-click on an extension panel item.
    /// Fires `panel_double_click` event (same arg format as `panel_select`).
    pub fn handle_ext_panel_double_click(&mut self) {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => return,
        };
        if let Some((si, item_idx)) = self.ext_panel_flat_to_section(self.ext_panel_selected) {
            if item_idx != usize::MAX {
                let reg = self.ext_panels.get(&panel_name).cloned();
                if let Some(reg) = reg {
                    if let Some(section) = reg.sections.get(si) {
                        let key = (panel_name.clone(), section.clone());
                        let id = self
                            .ext_panel_items
                            .get(&key)
                            .and_then(|items| items.get(item_idx))
                            .map(|item| item.id.clone())
                            .unwrap_or_default();
                        let arg = format!(
                            "{}|{}|{}||{}",
                            panel_name, section, id, self.ext_panel_selected
                        );
                        self.plugin_event("panel_double_click", &arg);
                    }
                }
            }
        }
    }

    /// Open a context menu for an extension panel item.
    /// Fires `panel_context_menu` with the selected item info.
    pub fn open_ext_panel_context_menu(&mut self, x: u16, y: u16) {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => return,
        };
        if let Some((si, item_idx)) = self.ext_panel_flat_to_section(self.ext_panel_selected) {
            let reg = self.ext_panels.get(&panel_name).cloned();
            if let Some(reg) = reg {
                if let Some(section) = reg.sections.get(si) {
                    let key = (panel_name.clone(), section.clone());
                    let id = if item_idx != usize::MAX {
                        self.ext_panel_items
                            .get(&key)
                            .and_then(|items| items.get(item_idx))
                            .map(|item| item.id.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let arg = format!(
                        "{}|{}|{}||{}",
                        panel_name, section, id, self.ext_panel_selected
                    );
                    self.plugin_event("panel_context_menu", &arg);
                }
            }
        }
        let _ = (x, y); // Position reserved for future native menu rendering.
    }

    /// Handle keyboard input for the extension panel input field.
    /// Returns `true` if the key was consumed.
    pub fn handle_ext_panel_input_key(
        &mut self,
        key: &str,
        _ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => {
                self.ext_panel_input_active = false;
                return true;
            }
        };

        match key {
            "Escape" => {
                self.ext_panel_input_active = false;
            }
            "Return" => {
                // Fire panel_input event with the current text, then deactivate.
                let text = self
                    .ext_panel_input_text
                    .get(&panel_name)
                    .cloned()
                    .unwrap_or_default();
                let arg = format!("{}|||{}|", panel_name, text);
                self.plugin_event("panel_input", &arg);
                self.ext_panel_input_active = false;
            }
            "BackSpace" => {
                if let Some(text) = self.ext_panel_input_text.get_mut(&panel_name) {
                    text.pop();
                }
                // Fire panel_input on every change for live filtering.
                let text = self
                    .ext_panel_input_text
                    .get(&panel_name)
                    .cloned()
                    .unwrap_or_default();
                let arg = format!("{}|||{}|", panel_name, text);
                self.plugin_event("panel_input", &arg);
            }
            _ => {
                if let Some(ch) = unicode {
                    if !ch.is_control() {
                        self.ext_panel_input_text
                            .entry(panel_name.clone())
                            .or_default()
                            .push(ch);
                        // Fire panel_input on every change for live filtering.
                        let text = self
                            .ext_panel_input_text
                            .get(&panel_name)
                            .cloned()
                            .unwrap_or_default();
                        let arg = format!("{}|||{}|", panel_name, text);
                        self.plugin_event("panel_input", &arg);
                    }
                }
            }
        }
        true
    }

    // ── Panel hover popup methods ──────────────────────────────────────────

    /// Show a hover popup with rendered markdown for a sidebar panel item.
    pub fn show_panel_hover(
        &mut self,
        panel_name: &str,
        item_id: &str,
        item_index: usize,
        markdown: &str,
    ) {
        let rendered = crate::core::markdown::render_markdown(markdown);
        let links = Self::extract_hover_links(&rendered);
        // Dismiss any active editor hover to avoid overlapping popups.
        self.dismiss_editor_hover();
        self.panel_hover = Some(PanelHoverPopup {
            rendered,
            links,
            panel_name: panel_name.to_string(),
            item_id: item_id.to_string(),
            item_index,
        });
    }

    /// Schedule a delayed dismiss of the hover popup (250ms grace period).
    /// The popup stays visible until `poll_panel_hover` sees the deadline pass.
    /// If the mouse moves back onto the popup or item, call `cancel_panel_hover_dismiss`.
    pub fn dismiss_panel_hover(&mut self) {
        if self.panel_hover.is_some() && self.panel_hover_dismiss_at.is_none() {
            self.panel_hover_dismiss_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
        }
        // Always clear dwell so a new hover won't restart on the old item.
        self.panel_hover_dwell = None;
    }

    /// Immediately dismiss the hover popup with no delay.
    pub fn dismiss_panel_hover_now(&mut self) {
        self.panel_hover = None;
        self.panel_hover_dwell = None;
        self.panel_hover_dismiss_at = None;
    }

    /// Cancel a pending delayed dismiss (mouse returned to popup or item).
    pub fn cancel_panel_hover_dismiss(&mut self) {
        self.panel_hover_dismiss_at = None;
    }

    /// Track mouse movement over a sidebar panel item for dwell detection.
    /// Returns true if the dwell state changed (item changed).
    pub fn panel_hover_mouse_move(
        &mut self,
        panel_name: &str,
        item_id: &str,
        item_index: usize,
    ) -> bool {
        let _ = item_id;
        // If mouse returned to the item that spawned the current popup, cancel dismiss.
        if let Some(ref ph) = self.panel_hover {
            if ph.panel_name == panel_name && ph.item_index == item_index {
                self.panel_hover_dismiss_at = None;
                return false;
            }
        }
        if let Some((ref pn, idx, _)) = self.panel_hover_dwell {
            if pn == panel_name && idx == item_index {
                // Same dwell item. Only cancel dismiss if this item owns
                // the current popup (not if a *different* popup is lingering).
                let owns_popup = self
                    .panel_hover
                    .as_ref()
                    .is_some_and(|ph| ph.panel_name == panel_name && ph.item_index == item_index);
                if owns_popup {
                    self.panel_hover_dismiss_at = None;
                }
                return false; // Same item, dwell still running
            }
        }
        // Different item — schedule delayed dismiss for the active popup
        // (so it lingers while the user moves the mouse toward it) and start
        // dwell tracking on the new item.
        if self.panel_hover.is_some() && self.panel_hover_dismiss_at.is_none() {
            self.panel_hover_dismiss_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
        }
        // If no popup is showing, clear any stale dismiss.
        if self.panel_hover.is_none() {
            self.panel_hover_dismiss_at = None;
        }
        self.panel_hover_dwell = Some((
            panel_name.to_string(),
            item_index,
            std::time::Instant::now(),
        ));
        true
    }

    /// Called from poll/tick loops. Handles dwell-to-show and delayed dismiss.
    /// Returns true if a redraw is needed.
    pub fn poll_panel_hover(&mut self) -> bool {
        if self.settings.hover_delay == 0 {
            return false;
        }
        // Check delayed dismiss deadline.
        if let Some(deadline) = self.panel_hover_dismiss_at {
            if std::time::Instant::now() >= deadline {
                self.panel_hover = None;
                self.panel_hover_dismiss_at = None;
                return true; // redraw to remove popup
            }
        }

        let Some((ref panel_name, item_index, started)) = self.panel_hover_dwell else {
            return false;
        };
        if self.panel_hover.is_some() {
            return false; // Already showing
        }
        if started.elapsed() < std::time::Duration::from_millis(self.settings.hover_delay as u64) {
            return false; // Not yet
        }
        let panel_name = panel_name.clone();

        // Native source control panel hovers.
        if panel_name == "source_control" {
            if let Some(md) = self.sc_hover_markdown(item_index) {
                self.show_panel_hover(&panel_name, "", item_index, &md);
                return true;
            }
            self.panel_hover_dwell = None;
            return false;
        }

        // Extension panel: resolve item_id and check plugin registry.
        let item_id = self.resolve_panel_hover_item_id(&panel_name, item_index);
        let md = self
            .panel_hover_registry
            .get(&(panel_name.clone(), item_id.clone()))
            .cloned();
        if let Some(md) = md {
            self.show_panel_hover(&panel_name, &item_id, item_index, &md);
            return true;
        }
        // Prevent re-polling: clear dwell so we don't keep trying every tick.
        self.panel_hover_dwell = None;
        false
    }

    /// Generate hover markdown for a Source Control panel item at the given flat index.
    fn sc_hover_markdown(&self, flat_index: usize) -> Option<String> {
        let (section, idx) = self.sc_flat_to_section_idx(flat_index);

        // Section headers: show branch info on the "Staged Changes" header (section 0)
        if idx == usize::MAX {
            if section == 0 {
                // Branch info hover
                return self.sc_hover_branch_info();
            }
            return None; // Other headers: no hover
        }

        match section {
            // Staged/Unstaged file items
            0 | 1 => {
                let is_staged = section == 0;
                let files: Vec<&git::FileStatus> = if is_staged {
                    self.sc_file_statuses
                        .iter()
                        .filter(|f| f.staged.is_some())
                        .collect()
                } else {
                    self.sc_file_statuses
                        .iter()
                        .filter(|f| f.unstaged.is_some())
                        .collect()
                };
                let file = files.get(idx)?;
                self.sc_hover_file(file, is_staged)
            }
            // Log items
            3 => {
                let entry = self.sc_log.get(idx)?;
                self.sc_hover_log_entry(entry)
            }
            _ => None,
        }
    }

    /// Branch info hover (shown on the Staged Changes section header).
    fn sc_hover_branch_info(&self) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        let branch = git::current_branch(&cwd)?;
        let tracking = git::tracking_branch(&cwd).unwrap_or_else(|| "none".to_string());
        let mut md = format!("### {} `{}`\n\n", "\u{e725}", branch); // nf-dev-git_branch
        md.push_str(&format!("**Remote:** `{}`\n\n", tracking));
        if self.sc_ahead > 0 || self.sc_behind > 0 {
            md.push_str(&format!(
                "\u{2191}{} \u{2193}{}",
                self.sc_ahead, self.sc_behind
            ));
            if self.sc_ahead > 0 {
                md.push_str(" — commits to push");
            }
            if self.sc_behind > 0 {
                md.push_str(" — commits to pull");
            }
            md.push('\n');
        } else {
            md.push_str("Up to date with remote\n");
        }
        Some(md)
    }

    /// File hover: show status and diff stats.
    fn sc_hover_file(&self, file: &git::FileStatus, staged: bool) -> Option<String> {
        let status = if staged {
            file.staged.unwrap_or(git::StatusKind::Modified)
        } else {
            file.unstaged.unwrap_or(git::StatusKind::Modified)
        };
        let status_label = match status {
            git::StatusKind::Added => "Added",
            git::StatusKind::Modified => "Modified",
            git::StatusKind::Deleted => "Deleted",
            git::StatusKind::Renamed => "Renamed",
            git::StatusKind::Untracked => "Untracked",
        };
        let mut md = format!("### {}\n\n", file.path);
        md.push_str(&format!(
            "**Status:** {} ({})\n\n",
            status_label,
            if staged { "staged" } else { "unstaged" }
        ));
        // Get diff stats (blocking but fast for a single file)
        let cwd = std::env::current_dir().ok()?;
        if let Some(stat) = git::diff_stat_file(&cwd, &file.path, staged) {
            md.push_str("```\n");
            md.push_str(&stat);
            md.push_str("\n```\n");
        }
        Some(md)
    }

    /// Log entry hover: show commit details.
    fn sc_hover_log_entry(&self, entry: &git::GitLogEntry) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        if let Some(detail) = git::commit_detail(&cwd, &entry.hash) {
            let mut md = String::new();
            // If we can build a commit URL, make the hash a clickable link.
            if let Some(url) = git::commit_url(&cwd, &detail.hash) {
                md.push_str(&format!("### [{}]({})\n\n", detail.hash, url));
            } else {
                md.push_str(&format!("### `{}`\n\n", detail.hash));
            }
            md.push_str(&format!("**Author:** {}\n\n", detail.author));
            md.push_str(&format!("**Date:** {}\n\n", detail.date));
            if !detail.message.is_empty() {
                md.push_str(&detail.message);
                md.push_str("\n\n");
            }
            if !detail.stat.is_empty() {
                md.push_str("```\n");
                md.push_str(&detail.stat);
                md.push_str("\n```\n");
            }
            Some(md)
        } else {
            // Fallback to basic info
            Some(format!("### `{}`\n\n{}\n", entry.hash, entry.message))
        }
    }

    /// Resolve the item_id for a given panel name and flat index.
    fn resolve_panel_hover_item_id(&self, panel_name: &str, flat_index: usize) -> String {
        let Some(reg) = self.ext_panels.get(panel_name) else {
            return String::new();
        };
        let expanded = self.ext_panel_sections_expanded.get(panel_name);
        let mut idx = 0usize;
        for (si, section_name) in reg.sections.iter().enumerate() {
            if idx == flat_index {
                return String::new(); // It's a section header
            }
            idx += 1;
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.to_string(), section_name.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    for item in items {
                        if idx == flat_index {
                            return item.id.clone();
                        }
                        idx += 1;
                    }
                }
            }
        }
        String::new()
    }

    // ── Editor hover popup ────────────────────────────────────────────────────

    /// Trigger the editor hover popup at the current cursor position.
    /// Assembles content from multiple providers: diagnostics, annotations,
    /// plugin hover content, and LSP hover. Also requests LSP hover async.
    pub fn trigger_editor_hover_at_cursor(&mut self) {
        let line = self.cursor().line;
        let col = self.cursor().col;
        self.show_editor_hover_at(line, col, true, true);
    }

    /// Check if any diagnostic touches the given line.
    pub fn has_diagnostic_on_line(&self, line: usize) -> bool {
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                return diags.iter().any(|d| {
                    let sl = d.range.start.line as usize;
                    let el = d.range.end.line as usize;
                    line >= sl && line <= el
                });
            }
        }
        false
    }

    /// Trigger editor hover for a diagnostic gutter click on the given line.
    /// Shows ALL diagnostics that touch this line, regardless of column.
    pub fn trigger_editor_hover_for_line(&mut self, line: usize) {
        let mut sections: Vec<String> = Vec::new();
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                for diag in diags {
                    let start_line = diag.range.start.line as usize;
                    let end_line = diag.range.end.line as usize;
                    if line >= start_line && line <= end_line {
                        let severity = match diag.severity {
                            crate::core::lsp::DiagnosticSeverity::Error => "Error",
                            crate::core::lsp::DiagnosticSeverity::Warning => "Warning",
                            crate::core::lsp::DiagnosticSeverity::Information => "Info",
                            crate::core::lsp::DiagnosticSeverity::Hint => "Hint",
                        };
                        let source_str = diag
                            .source
                            .as_deref()
                            .map(|s| format!(" ({})", s))
                            .unwrap_or_default();
                        sections.push(format!(
                            "**{}**{}\n\n`{}`",
                            severity, source_str, diag.message
                        ));
                    }
                }
            }
        }
        if !sections.is_empty() {
            let combined = sections.join("\n\n---\n\n");
            self.show_editor_hover(
                line,
                0,
                &combined,
                EditorHoverSource::Diagnostic,
                true,
                false,
            );
        }
    }

    /// Assemble and show the editor hover popup at a given buffer position.
    /// If `request_lsp` is true, also fires an LSP hover request (async).
    /// If `take_focus` is true, the popup grabs keyboard focus (j/k scroll, Tab links).
    pub fn show_editor_hover_at(
        &mut self,
        line: usize,
        col: usize,
        request_lsp: bool,
        take_focus: bool,
    ) {
        self.show_editor_hover_at_inner(line, col, request_lsp, take_focus, true);
    }

    /// Inner implementation — `include_annotations` controls whether annotation
    /// hover content is included (false for mouse dwell over code text, true for
    /// keyboard triggers and mouse dwell over ghost text).
    fn show_editor_hover_at_inner(
        &mut self,
        line: usize,
        col: usize,
        request_lsp: bool,
        take_focus: bool,
        include_annotations: bool,
    ) {
        let mut sections: Vec<(EditorHoverSource, String)> = Vec::new();

        // 1. Diagnostics at this position
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                for diag in diags {
                    let start_line = diag.range.start.line as usize;
                    let end_line = diag.range.end.line as usize;
                    let start_col = diag.range.start.character as usize;
                    let end_col = diag.range.end.character as usize;
                    let in_range = if start_line == end_line {
                        line == start_line && col >= start_col && col <= end_col
                    } else {
                        (line == start_line && col >= start_col)
                            || (line == end_line && col <= end_col)
                            || (line > start_line && line < end_line)
                    };
                    if in_range {
                        let severity = match diag.severity {
                            crate::core::lsp::DiagnosticSeverity::Error => "Error",
                            crate::core::lsp::DiagnosticSeverity::Warning => "Warning",
                            crate::core::lsp::DiagnosticSeverity::Information => "Info",
                            crate::core::lsp::DiagnosticSeverity::Hint => "Hint",
                        };
                        let source_str = diag
                            .source
                            .as_deref()
                            .map(|s| format!(" ({})", s))
                            .unwrap_or_default();
                        let md = format!("**{}**{}\n\n`{}`", severity, source_str, diag.message);
                        sections.push((EditorHoverSource::Diagnostic, md));
                    }
                }
            }
        }

        // 2. Plugin hover content for this line (only when over annotation area)
        if include_annotations {
            if let Some(md) = self.editor_hover_content.get(&line) {
                sections.push((EditorHoverSource::Annotation, md.clone()));
            }
        }

        // 3. Line annotation text (simple inline blame, etc.)
        if include_annotations && sections.is_empty() {
            if let Some(annotation) = self.line_annotations.get(&line) {
                if !annotation.is_empty() {
                    // Query plugin hover providers for annotation content
                    let md = format!("`{}`", annotation.trim());
                    sections.push((EditorHoverSource::Annotation, md));
                }
            }
        }

        // 4. Existing LSP hover text (if already available)
        if let Some(hover_text) = &self.lsp_hover_text {
            sections.push((EditorHoverSource::Lsp, hover_text.clone()));
        }

        // Build the popup if we have content
        let has_lsp_section = sections
            .iter()
            .any(|(s, _)| matches!(s, EditorHoverSource::Lsp));
        let is_annotation_only = !sections.is_empty()
            && sections
                .iter()
                .all(|(s, _)| matches!(s, EditorHoverSource::Annotation));
        if !sections.is_empty() {
            let combined = sections
                .iter()
                .map(|(_, md)| md.as_str())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            let source = sections[0].0.clone();
            // Annotation-only hovers don't auto-focus — user clicks to focus.
            let focus = take_focus && !is_annotation_only;
            self.show_editor_hover(line, col, &combined, source, focus, false);
        } else if take_focus {
            self.editor_hover_has_focus = true;
        }

        // Request LSP hover only if we don't already have LSP content and
        // the popup isn't purely annotation-sourced (avoids LSP null response
        // dismissing the annotation popup).
        if request_lsp && !is_annotation_only && !has_lsp_section {
            // For mouse hover: skip if LSP already returned null for this position.
            if !take_focus && self.lsp_hover_null_pos == Some((line, col)) {
                return;
            }
            self.lsp_hover_request_pos = Some((line, col));
            let prev_pending = self.lsp_pending_hover;
            self.lsp_request_hover_at(line, col);
            let sent_new =
                self.lsp_pending_hover != prev_pending && self.lsp_pending_hover.is_some();
            if sent_new && take_focus && self.editor_hover.is_none() {
                // Explicit keyboard hover (gh/:hover) — show "Loading..." immediately.
                self.show_editor_hover(
                    line,
                    col,
                    "Loading...",
                    EditorHoverSource::Lsp,
                    true,
                    false,
                );
                // Auto-dismiss after 3s if LSP never responds.
                self.editor_hover_dismiss_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            // Mouse hover: no "Loading..." — popup appears only if LSP returns content.
        }
    }

    /// Show an editor hover popup with the given markdown content.
    /// If `take_focus` is true, the popup grabs keyboard focus (for `gh` / `:hover`).
    pub fn show_editor_hover(
        &mut self,
        anchor_line: usize,
        anchor_col: usize,
        markdown: &str,
        source: EditorHoverSource,
        take_focus: bool,
        add_goto_links: bool,
    ) {
        let mut rendered = crate::core::markdown::render_markdown(markdown);
        let mut links = Self::extract_hover_links(&rendered);

        // Append "Go to" navigation links after actual LSP content (vim mode only).
        if add_goto_links && !self.is_vscode_mode() {
            let goto = self.lsp_goto_links();
            if !goto.is_empty() {
                use crate::core::markdown::{MdSpan, MdStyle};
                // Separator line.
                rendered.lines.push(String::new());
                rendered.spans.push(Vec::new());
                rendered.code_highlights.push(Vec::new());
                // Build: "Go to Definition (:gd) | Type Definition (:gy) | ..."
                // "Go to" is default fg; labels are link-colored and clickable.
                let nav_line_idx = rendered.lines.len();
                let mut nav_text = String::from("Go to ");
                let mut nav_spans = Vec::new();
                for (i, (label, keybind, url)) in goto.iter().enumerate() {
                    if i > 0 {
                        nav_text.push_str(" | ");
                    }
                    let start = nav_text.len();
                    nav_text.push_str(label);
                    let end = nav_text.len();
                    nav_spans.push(MdSpan {
                        start_byte: start,
                        end_byte: end,
                        style: MdStyle::Link,
                    });
                    links.push((nav_line_idx, start, end, url.to_string()));
                    nav_text.push_str(&format!(" (:{})", keybind));
                }
                rendered.lines.push(nav_text);
                rendered.spans.push(nav_spans);
                rendered.code_highlights.push(Vec::new());
            }
        }

        let popup_width = rendered
            .lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(10)
            .clamp(10, 80);
        let (frozen_scroll_top, frozen_scroll_left) = {
            let v = self.view();
            (v.scroll_top, v.scroll_left)
        };
        // Dismiss any active panel hover to avoid overlapping popups.
        self.dismiss_panel_hover_now();
        self.editor_hover = Some(EditorHoverPopup {
            rendered,
            links,
            anchor_line,
            anchor_col,
            source,
            scroll_top: 0,
            focused_link: None,
            popup_width,
            frozen_scroll_top,
            frozen_scroll_left,
            selection: None,
        });
        if take_focus {
            self.editor_hover_has_focus = true;
        }
    }

    /// Dismiss the editor hover popup.
    pub fn dismiss_editor_hover(&mut self) {
        self.editor_hover = None;
        self.editor_hover_has_focus = false;
        self.editor_hover_dwell = None;
        self.editor_hover_dismiss_at = None;
        self.lsp_hover_text = None;
    }

    /// Dismiss editor hover with a delay (for mouse leave events).
    #[allow(dead_code)]
    pub fn dismiss_editor_hover_delayed(&mut self) {
        if self.editor_hover.is_some() && self.editor_hover_dismiss_at.is_none() {
            self.editor_hover_dismiss_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
        }
        self.editor_hover_dwell = None;
    }

    /// Cancel a pending delayed editor hover dismiss.
    #[allow(dead_code)]
    pub fn cancel_editor_hover_dismiss(&mut self) {
        self.editor_hover_dismiss_at = None;
    }

    /// Handle keyboard input when the editor hover popup has focus.
    pub fn handle_editor_hover_key(&mut self, key: &str, ctrl: bool) {
        match key {
            "y" | "Y" => {
                self.copy_hover_selection();
            }
            "c" if ctrl => {
                self.copy_hover_selection();
            }
            "Escape" | "q" => {
                self.dismiss_editor_hover();
            }
            "Tab" => {
                // Cycle to next link
                if let Some(hover) = &mut self.editor_hover {
                    if !hover.links.is_empty() {
                        hover.focused_link = Some(match hover.focused_link {
                            Some(i) => (i + 1) % hover.links.len(),
                            None => 0,
                        });
                    }
                }
            }
            "ISO_Left_Tab" | "BackTab" => {
                // Cycle to previous link
                if let Some(hover) = &mut self.editor_hover {
                    if !hover.links.is_empty() {
                        hover.focused_link = Some(match hover.focused_link {
                            Some(0) | None => hover.links.len() - 1,
                            Some(i) => i - 1,
                        });
                    }
                }
            }
            "Return" => {
                // Open focused link
                let url = self.editor_hover.as_ref().and_then(|h| {
                    h.focused_link
                        .and_then(|i| h.links.get(i).map(|(_, _, _, u)| u.clone()))
                });
                if let Some(url) = url {
                    if url.starts_with("command:") {
                        self.execute_hover_goto(&url);
                    } else {
                        self.open_url(&url);
                        self.dismiss_editor_hover();
                    }
                } else {
                    self.dismiss_editor_hover();
                }
            }
            "j" | "Down" => {
                // Scroll down — stop when last line is visible
                if let Some(hover) = &mut self.editor_hover {
                    let max_scroll = hover.rendered.lines.len().saturating_sub(20);
                    if hover.scroll_top < max_scroll {
                        hover.scroll_top += 1;
                    }
                }
            }
            "k" | "Up" => {
                // Scroll up
                if let Some(hover) = &mut self.editor_hover {
                    if hover.scroll_top > 0 {
                        hover.scroll_top -= 1;
                    }
                }
            }
            // Ignore bare modifier keys (GTK sends these as separate key events)
            "Control_L" | "Control_R" | "Shift_L" | "Shift_R" | "Alt_L" | "Alt_R" | "Super_L"
            | "Super_R" | "Meta_L" | "Meta_R" | "ISO_Level3_Shift" => {}
            _ => {
                // Any other key dismisses and passes through
                self.dismiss_editor_hover();
            }
        }
    }

    /// Track mouse movement for editor hover dwell detection.
    /// Call from backends on mouse motion over the editor area.
    /// Only triggers on word characters (identifiers), not whitespace or operators.
    /// Called by backends when the mouse moves over the editor area.
    /// `mouse_on_popup` should be true if the mouse is currently over the hover popup rect.
    pub fn editor_hover_mouse_move(&mut self, line: usize, col: usize, mouse_on_popup: bool) {
        if self.settings.hover_delay == 0 {
            return;
        }
        // If hover popup is already visible and focused, don't interfere
        if self.editor_hover_has_focus {
            return;
        }
        // Find the word boundaries under the cursor (if any)
        let (word_range, line_char_len) = {
            let buf = self.buffer();
            if line < buf.len_lines() {
                let line_text: String = buf.content.line(line).chars().collect();
                let chars: Vec<char> = line_text.chars().collect();
                let char_len =
                    chars
                        .len()
                        .saturating_sub(if chars.last() == Some(&'\n') { 1 } else { 0 });
                let wr = if col < chars.len() && (chars[col].is_alphanumeric() || chars[col] == '_')
                {
                    let mut start = col;
                    while start > 0
                        && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_')
                    {
                        start -= 1;
                    }
                    let mut end = col + 1;
                    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                        end += 1;
                    }
                    Some((start, end))
                } else {
                    None
                };
                (wr, char_len)
            } else {
                (None, 0)
            }
        };

        // Annotation hover content only counts when the mouse is past the end
        // of the actual line text (i.e. over the ghost text region).
        let on_annotation = col >= line_char_len
            && (self.editor_hover_content.contains_key(&line)
                || self.line_annotations.contains_key(&line));

        // Check if we're on the same word as the current popup
        if let Some(hover) = &self.editor_hover {
            // If popup is anchored to this line and mouse is on annotation, keep it
            if hover.anchor_line == line && on_annotation {
                return;
            }
            if let Some((start, end)) = word_range {
                if hover.anchor_line == line && hover.anchor_col >= start && hover.anchor_col < end
                {
                    // Still on the popup's word — nothing to do
                    return;
                }
            }
            // Not on the popup's word — but if mouse is on the popup itself, keep it
            if mouse_on_popup {
                return;
            }
            // Off both word and popup — dismiss (no cooldown for natural mouse-off)
            self.editor_hover = None;
            self.editor_hover_has_focus = false;
            self.editor_hover_dwell = None;
            self.editor_hover_dismiss_at = None;
            self.lsp_hover_text = None;
            return;
        }

        // No popup visible — handle dwell logic
        if word_range.is_none() && !on_annotation {
            self.editor_hover_dwell = None;
            return;
        }
        // Check if we're still on the same word/line as the current dwell
        if let Some((dl, dc, _)) = &self.editor_hover_dwell {
            if *dl == line {
                // If mouse is on annotation area and no word boundary, stay dwelling
                if on_annotation && word_range.is_none() {
                    return;
                }
                if let Some((start, end)) = word_range {
                    if *dc >= start && *dc < end {
                        // Same word — keep dwelling
                        return;
                    }
                }
            }
        }
        // New word — start fresh dwell timer and clear null-hover suppression.
        self.lsp_hover_null_pos = None;
        self.editor_hover_dwell = Some((line, col, std::time::Instant::now()));
    }

    /// Scroll the editor hover popup by the given delta (positive = down, negative = up).
    /// Returns true if the popup was scrolled.
    pub fn editor_hover_scroll(&mut self, delta: i32) -> bool {
        if let Some(hover) = &mut self.editor_hover {
            let max_scroll = hover.rendered.lines.len().saturating_sub(20);
            if delta > 0 {
                let new = (hover.scroll_top + delta as usize).min(max_scroll);
                if new != hover.scroll_top {
                    hover.scroll_top = new;
                    return true;
                }
            } else {
                let new = hover.scroll_top.saturating_sub((-delta) as usize);
                if new != hover.scroll_top {
                    hover.scroll_top = new;
                    return true;
                }
            }
        }
        false
    }

    /// Give the editor hover popup keyboard focus (e.g. on click).
    pub fn editor_hover_focus(&mut self) {
        if self.editor_hover.is_some() {
            self.editor_hover_has_focus = true;
        }
    }

    /// Extract the selected text from the editor hover popup (or all text if no selection).
    /// Returns `None` if there is no hover popup or content is empty.
    pub fn hover_selection_text(&self) -> Option<String> {
        let hover = self.editor_hover.as_ref()?;
        let text = if let Some(ref sel) = hover.selection {
            sel.extract_text(&hover.rendered.lines)
        } else {
            hover.rendered.lines.join("\n")
        };
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Copy the selected text from the editor hover popup to the clipboard.
    /// If no selection is active, copies all popup text.
    /// Uses the engine's `clipboard_write` callback (set by TUI backend).
    /// GTK backend should call `hover_selection_text()` and use its own clipboard.
    pub fn copy_hover_selection(&mut self) {
        let text = match self.hover_selection_text() {
            Some(t) => t,
            None => return,
        };
        if let Some(ref cb) = self.clipboard_write {
            if cb(&text).is_ok() {
                self.message = "Hover text copied".to_string();
                return;
            }
        }
        self.message = "Clipboard unavailable".to_string();
    }

    /// Start a text selection in the editor hover popup at the given content position.
    pub fn editor_hover_start_selection(&mut self, line: usize, col: usize) {
        if let Some(hover) = &mut self.editor_hover {
            hover.selection = Some(HoverSelection {
                anchor_line: line,
                anchor_col: col,
                active_line: line,
                active_col: col,
            });
        }
    }

    /// Extend the text selection in the editor hover popup to the given content position.
    pub fn editor_hover_extend_selection(&mut self, line: usize, col: usize) {
        if let Some(hover) = &mut self.editor_hover {
            if let Some(sel) = &mut hover.selection {
                sel.active_line = line;
                sel.active_col = col;
            }
        }
    }

    /// Poll editor hover dwell and delayed dismiss timers.
    /// Call from backends in the event loop tick.
    pub fn poll_editor_hover(&mut self) -> bool {
        if self.settings.hover_delay == 0 {
            return false;
        }
        let mut changed = false;
        // Check dwell timeout
        if let Some((line, col, start)) = self.editor_hover_dwell {
            if start.elapsed() >= std::time::Duration::from_millis(self.settings.hover_delay as u64)
            {
                self.editor_hover_dwell = None;
                // Re-validate position: on a word character or annotation ghost text
                let (on_annotation, on_word) = {
                    let buf = self.buffer();
                    let line_char_len = if line < buf.len_lines() {
                        let lt: String = buf.content.line(line).chars().collect();
                        let chars: Vec<char> = lt.chars().collect();
                        chars
                            .len()
                            .saturating_sub(if chars.last() == Some(&'\n') { 1 } else { 0 })
                    } else {
                        0
                    };
                    let ann = col >= line_char_len
                        && (self.editor_hover_content.contains_key(&line)
                            || self.line_annotations.contains_key(&line));
                    let word = if !ann && line < buf.len_lines() {
                        let line_text: String = buf.content.line(line).chars().collect();
                        line_text
                            .chars()
                            .nth(col)
                            .is_some_and(|c| c.is_alphanumeric() || c == '_')
                    } else {
                        false
                    };
                    (ann, word)
                };
                if on_annotation || on_word {
                    self.show_editor_hover_at_inner(line, col, true, false, on_annotation);
                    changed = true;
                }
            }
        }
        // Check delayed dismiss
        if let Some(deadline) = self.editor_hover_dismiss_at {
            if std::time::Instant::now() >= deadline {
                self.dismiss_editor_hover();
                changed = true;
            }
        }
        changed
    }

    /// Check if there's a diagnostic at the given position.
    #[allow(dead_code)]
    fn has_diagnostic_at(&self, line: usize, col: usize) -> bool {
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                for diag in diags {
                    let sl = diag.range.start.line as usize;
                    let el = diag.range.end.line as usize;
                    let sc = diag.range.start.character as usize;
                    let ec = diag.range.end.character as usize;
                    let in_range = if sl == el {
                        line == sl && col >= sc && col <= ec
                    } else {
                        (line == sl && col >= sc)
                            || (line == el && col <= ec)
                            || (line > sl && line < el)
                    };
                    if in_range {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Open a URL in the default browser.
    fn open_url(&self, url: &str) {
        if !is_safe_url(url) {
            return;
        }
        #[cfg(not(test))]
        {
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open")
                    .arg(url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = std::process::Command::new("xdg-open")
                    .arg(url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        }
    }

    /// Get the file path of the active buffer (if it has one).
    fn active_buffer_path(&self) -> Option<PathBuf> {
        self.buffer_manager
            .get(self.active_window().buffer_id)
            .and_then(|bs| bs.file_path.clone())
    }

    /// Handle LSP hover response by updating the editor hover popup.
    /// Called when the hover response arrives asynchronously.
    pub fn update_editor_hover_with_lsp(&mut self, hover_text: &str) {
        if let Some(hover) = &self.editor_hover {
            let anchor_line = hover.anchor_line;
            let anchor_col = hover.anchor_col;
            let had_focus = self.editor_hover_has_focus;

            // Rebuild: diagnostics at this position + new LSP text (replaces any old LSP content)
            let mut sections: Vec<String> = Vec::new();

            // Re-collect diagnostics for this anchor position
            if let Some(path) = self.active_buffer_path() {
                if let Some(diags) = self.lsp_diagnostics.get(&path) {
                    for diag in diags {
                        let sl = diag.range.start.line as usize;
                        let el = diag.range.end.line as usize;
                        let sc = diag.range.start.character as usize;
                        let ec = diag.range.end.character as usize;
                        let in_range = if sl == el {
                            anchor_line == sl && anchor_col >= sc && anchor_col <= ec
                        } else {
                            (anchor_line == sl && anchor_col >= sc)
                                || (anchor_line == el && anchor_col <= ec)
                                || (anchor_line > sl && anchor_line < el)
                        };
                        if in_range {
                            let severity = match diag.severity {
                                crate::core::lsp::DiagnosticSeverity::Error => "Error",
                                crate::core::lsp::DiagnosticSeverity::Warning => "Warning",
                                crate::core::lsp::DiagnosticSeverity::Information => "Info",
                                crate::core::lsp::DiagnosticSeverity::Hint => "Hint",
                            };
                            let source_str = diag
                                .source
                                .as_deref()
                                .map(|s| format!(" ({})", s))
                                .unwrap_or_default();
                            sections.push(format!(
                                "**{}**{}\n\n`{}`",
                                severity, source_str, diag.message
                            ));
                        }
                    }
                }
            }

            // Add LSP hover text
            if !hover_text.is_empty() {
                sections.push(hover_text.to_string());
            }

            let combined = sections.join("\n\n---\n\n");
            self.show_editor_hover(
                anchor_line,
                anchor_col,
                &combined,
                EditorHoverSource::Lsp,
                had_focus,
                true,
            );
        } else {
            // No existing popup — create one from LSP content
            let line = self.cursor().line;
            let col = self.cursor().col;
            let had_focus = self.editor_hover_has_focus;
            self.show_editor_hover(
                line,
                col,
                hover_text,
                EditorHoverSource::Lsp,
                had_focus,
                true,
            );
        }
    }

    /// Handle keyboard input for the Extensions sidebar panel.
    /// Returns `true` if the key was consumed.
    pub fn handle_ext_sidebar_key(
        &mut self,
        key: &str,
        _ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        // Search input active — route printable chars to query
        if self.ext_sidebar_input_active {
            match key {
                "Escape" => {
                    self.ext_sidebar_input_active = false;
                }
                "BackSpace" => {
                    self.ext_sidebar_query.pop();
                    self.ext_sidebar_selected = 0;
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.ext_sidebar_query.push(ch);
                            self.ext_sidebar_selected = 0;
                        }
                    }
                }
            }
            return true;
        }

        match key {
            "q" | "Escape" => {
                self.ext_sidebar_has_focus = false;
                true
            }
            "/" => {
                self.ext_sidebar_input_active = true;
                true
            }
            "r" => {
                self.ext_refresh();
                true
            }
            "Tab" => {
                // Toggle the section the cursor is in
                let (in_installed, _) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    self.ext_sidebar_sections_expanded[0] = !self.ext_sidebar_sections_expanded[0];
                } else {
                    self.ext_sidebar_sections_expanded[1] = !self.ext_sidebar_sections_expanded[1];
                }
                true
            }
            "j" | "Down" => {
                let total = self.ext_flat_item_count();
                if total > 0 {
                    self.ext_sidebar_selected = (self.ext_sidebar_selected + 1).min(total - 1);
                }
                true
            }
            "k" | "Up" => {
                self.ext_sidebar_selected = self.ext_sidebar_selected.saturating_sub(1);
                true
            }
            "Return" => {
                self.ext_open_selected_readme();
                true
            }
            "i" => {
                // Install the selected extension
                let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = &m.name;
                        self.message =
                            format!("Extension '{name}' is already installed. Use d to remove.");
                    }
                } else {
                    let available = self.ext_available_items();
                    let avail_idx = idx;
                    if avail_idx < available.len() {
                        let base_url = self.resolve_registry_base_url(&available[avail_idx]);
                        let name = available[avail_idx].name.clone();
                        let display = if available[avail_idx].display_name.is_empty() {
                            name.clone()
                        } else {
                            available[avail_idx].display_name.clone()
                        };
                        self.ext_install_from_registry(&name);
                        // Try to open README after install
                        let readme_path = paths::vimcode_config_dir()
                            .join("extensions")
                            .join(&name)
                            .join("README.md");
                        let content = std::fs::read_to_string(&readme_path)
                            .ok()
                            .or_else(|| registry::fetch_readme(&base_url, &name));
                        if let Some(content) = content {
                            self.open_markdown_preview_in_tab(&content, &display);
                        }
                        // Move cursor to the newly installed item.
                        self.ext_sidebar_sections_expanded[0] = true;
                        let new_installed = self.ext_installed_items();
                        self.ext_sidebar_selected = new_installed
                            .iter()
                            .position(|m| m.name == name)
                            .unwrap_or(0);
                    }
                }
                true
            }
            "d" => {
                let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = m.name.clone();
                        self.ext_show_remove_dialog(&name);
                    }
                }
                true
            }
            "u" => {
                // Update the selected installed extension
                let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = m.name.clone();
                        if self.ext_has_update(&name) {
                            self.ext_update_one(&name);
                        } else {
                            self.message = format!("Extension '{name}' is already up to date");
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Returns the filtered list of installed extension manifests.
    pub fn ext_installed_items(&self) -> Vec<extensions::ExtensionManifest> {
        let q = self.ext_sidebar_query.to_lowercase();
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| self.extension_state.is_installed(&m.name))
            .filter(|m| {
                q.is_empty()
                    || m.name.to_lowercase().contains(&q)
                    || m.display_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Returns the filtered list of available (not yet installed) extension manifests.
    pub fn ext_available_items(&self) -> Vec<extensions::ExtensionManifest> {
        let q = self.ext_sidebar_query.to_lowercase();
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| !self.extension_state.is_installed(&m.name))
            .filter(|m| {
                q.is_empty()
                    || m.name.to_lowercase().contains(&q)
                    || m.display_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Total number of flat items in the sidebar (installed + available, respecting collapse).
    fn ext_flat_item_count(&self) -> usize {
        let installed = if self.ext_sidebar_sections_expanded[0] {
            self.ext_installed_items().len()
        } else {
            0
        };
        let available = if self.ext_sidebar_sections_expanded[1] {
            self.ext_available_items().len()
        } else {
            0
        };
        installed + available
    }

    /// Map the flat selected index to (section, index_within_section),
    /// accounting for collapsed sections.
    /// Returns `(true, idx)` for installed items, `(false, idx)` for available.
    fn ext_selected_to_section(&self, sel: usize) -> (bool, usize) {
        let installed_vis = if self.ext_sidebar_sections_expanded[0] {
            self.ext_installed_items().len()
        } else {
            0
        };
        if sel < installed_vis {
            (true, sel)
        } else {
            (false, sel - installed_vis)
        }
    }

    // ── Settings sidebar panel ──────────────────────────────────────────────────

    /// Row types for the settings flat list.
    /// Build the flat list of rows for the Settings sidebar.
    /// Includes both core settings and extension-declared settings.
    pub fn settings_flat_list(&self) -> Vec<SettingsRow> {
        use super::settings::{setting_categories, SETTING_DEFS};
        let cats = setting_categories();
        let query = self.settings_query.to_lowercase();
        let mut rows = Vec::new();

        // Core settings
        for (cat_idx, &cat) in cats.iter().enumerate() {
            let matching: Vec<usize> = SETTING_DEFS
                .iter()
                .enumerate()
                .filter(|(_, d)| d.category == cat)
                .filter(|(_, d)| {
                    query.is_empty()
                        || d.label.to_lowercase().contains(&query)
                        || d.key.to_lowercase().contains(&query)
                        || d.description.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect();

            if matching.is_empty() {
                continue;
            }

            rows.push(SettingsRow::CoreCategory(cat_idx));

            let collapsed =
                cat_idx < self.settings_collapsed.len() && self.settings_collapsed[cat_idx];
            if !collapsed {
                for def_idx in matching {
                    rows.push(SettingsRow::CoreSetting(def_idx));
                }
            }
        }

        // Extension settings — one section per installed extension that declares settings
        for manifest in self.ext_available_manifests() {
            if manifest.settings.is_empty() || !self.extension_state.is_installed(&manifest.name) {
                continue;
            }
            let matching: Vec<&super::extensions::ExtSettingDef> = manifest
                .settings
                .iter()
                .filter(|s| {
                    query.is_empty()
                        || s.label.to_lowercase().contains(&query)
                        || s.key.to_lowercase().contains(&query)
                        || s.description.to_lowercase().contains(&query)
                })
                .collect();
            if matching.is_empty() {
                continue;
            }

            rows.push(SettingsRow::ExtCategory(manifest.name.clone()));

            let collapsed = self
                .ext_settings_collapsed
                .get(&manifest.name)
                .copied()
                .unwrap_or(false);
            if !collapsed {
                for def in matching {
                    rows.push(SettingsRow::ExtSetting(
                        manifest.name.clone(),
                        def.key.clone(),
                    ));
                }
            }
        }

        rows
    }

    /// Load an extension's settings from disk, merging with manifest defaults.
    pub fn load_ext_settings(&mut self, ext_name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name == ext_name);
        let manifest = match manifest {
            Some(m) => m,
            None => return,
        };
        let mut values = HashMap::new();
        // Start with defaults from manifest
        for def in &manifest.settings {
            values.insert(def.key.clone(), def.default.clone());
        }
        // Overlay with saved values from disk
        let path = paths::vimcode_config_dir()
            .join("extensions")
            .join(ext_name)
            .join("settings.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(saved) = serde_json::from_str::<HashMap<String, String>>(&data) {
                for (k, v) in saved {
                    values.insert(k, v);
                }
            }
        }
        if !values.is_empty() {
            self.ext_settings.insert(ext_name.to_string(), values);
        }
    }

    /// Save an extension's settings to disk.
    fn save_ext_settings(&self, ext_name: &str) {
        if let Some(values) = self.ext_settings.get(ext_name) {
            let path = paths::vimcode_config_dir()
                .join("extensions")
                .join(ext_name)
                .join("settings.json");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(values) {
                let _ = std::fs::write(&path, json);
            }
        }
    }

    /// Get an extension setting value by `ext_name` and `key`.
    pub fn get_ext_setting(&self, ext_name: &str, key: &str) -> String {
        self.ext_settings
            .get(ext_name)
            .and_then(|m| m.get(key))
            .cloned()
            .unwrap_or_default()
    }

    /// Set an extension setting value and save to disk.
    pub fn set_ext_setting(&mut self, ext_name: &str, key: &str, value: &str) {
        self.ext_settings
            .entry(ext_name.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        self.save_ext_settings(ext_name);
    }

    /// Look up an `ExtSettingDef` by extension name and key.
    pub fn find_ext_setting_def(
        &self,
        ext_name: &str,
        key: &str,
    ) -> Option<super::extensions::ExtSettingDef> {
        self.ext_available_manifests()
            .into_iter()
            .find(|m| m.name == ext_name)
            .and_then(|m| m.settings.into_iter().find(|s| s.key == key))
    }

    /// Handle a key press while the settings panel has focus.
    pub fn handle_settings_key(&mut self, key: &str, _ctrl: bool, unicode: Option<char>) {
        use super::settings::{SettingType, SETTING_DEFS};

        // Search input active — route printable chars to query
        if self.settings_input_active {
            match key {
                "Escape" | "Return" => {
                    self.settings_input_active = false;
                }
                "BackSpace" => {
                    self.settings_query.pop();
                    self.settings_selected = 0;
                    self.settings_scroll_top = 0;
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.settings_query.push(ch);
                            self.settings_selected = 0;
                            self.settings_scroll_top = 0;
                        }
                    }
                }
            }
            return;
        }

        // Inline editing active — core setting (string/int)
        if let Some(def_idx) = self.settings_editing {
            match key {
                "Escape" => {
                    self.settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "Return" => {
                    let def = &SETTING_DEFS[def_idx];
                    let val = self.settings_edit_buf.clone();
                    if self.settings.set_value_str(def.key, &val).is_ok() {
                        let _ = self.settings.save();
                    }
                    self.settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "BackSpace" => {
                    self.settings_edit_buf.pop();
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let def = &SETTING_DEFS[def_idx];
                            if matches!(def.setting_type, SettingType::Integer { .. }) {
                                if ch.is_ascii_digit() {
                                    self.settings_edit_buf.push(ch);
                                }
                            } else {
                                self.settings_edit_buf.push(ch);
                            }
                        }
                    }
                }
            }
            return;
        }

        // Inline editing active — extension setting (string/int)
        if let Some((ref ext_name, ref ext_key)) = self.ext_settings_editing.clone() {
            match key {
                "Escape" => {
                    self.ext_settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "Return" => {
                    let val = self.settings_edit_buf.clone();
                    self.set_ext_setting(ext_name, ext_key, &val);
                    self.ext_settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "BackSpace" => {
                    self.settings_edit_buf.pop();
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let is_int = self
                                .find_ext_setting_def(ext_name, ext_key)
                                .is_some_and(|d| d.r#type == "integer");
                            if is_int {
                                if ch.is_ascii_digit() {
                                    self.settings_edit_buf.push(ch);
                                }
                            } else {
                                self.settings_edit_buf.push(ch);
                            }
                        }
                    }
                }
            }
            return;
        }

        // Normal navigation
        let flat = self.settings_flat_list();
        let total = flat.len();

        match key {
            "q" | "Escape" => {
                self.settings_has_focus = false;
            }
            "/" => {
                self.settings_input_active = true;
            }
            "j" | "Down" => {
                if total > 0 {
                    self.settings_selected = (self.settings_selected + 1).min(total - 1);
                }
            }
            "k" | "Up" => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            "Tab" | "Return" | "Space" | "l" | "Right" | "h" | "Left" => {
                if self.settings_selected < total {
                    match &flat[self.settings_selected] {
                        SettingsRow::CoreCategory(cat_idx) => {
                            let cat_idx = *cat_idx;
                            if matches!(key, "Tab" | "Return" | "Space")
                                && cat_idx < self.settings_collapsed.len()
                            {
                                self.settings_collapsed[cat_idx] =
                                    !self.settings_collapsed[cat_idx];
                            }
                        }
                        SettingsRow::CoreSetting(idx) => {
                            let idx = *idx;
                            let def = &SETTING_DEFS[idx];
                            match &def.setting_type {
                                SettingType::Bool => {
                                    if matches!(key, "Return" | "Space") {
                                        let cur = self.settings.get_value_str(def.key);
                                        let new_val = if cur == "true" { "false" } else { "true" };
                                        if self.settings.set_value_str(def.key, new_val).is_ok() {
                                            let _ = self.settings.save();
                                        }
                                    }
                                }
                                SettingType::Enum(options) => {
                                    let forward = matches!(key, "Return" | "Space" | "l" | "Right");
                                    let backward = matches!(key, "h" | "Left");
                                    if forward || backward {
                                        let cur = self.settings.get_value_str(def.key);
                                        if let Some(pos) =
                                            options.iter().position(|&o| o == cur.as_str())
                                        {
                                            let next = if forward {
                                                (pos + 1) % options.len()
                                            } else {
                                                (pos + options.len() - 1) % options.len()
                                            };
                                            if self
                                                .settings
                                                .set_value_str(def.key, options[next])
                                                .is_ok()
                                            {
                                                let _ = self.settings.save();
                                            }
                                        }
                                    }
                                }
                                SettingType::DynamicEnum(options_fn) => {
                                    let forward = matches!(key, "Return" | "Space" | "l" | "Right");
                                    let backward = matches!(key, "h" | "Left");
                                    if forward || backward {
                                        let options = options_fn();
                                        let cur = self.settings.get_value_str(def.key);
                                        if let Some(pos) = options.iter().position(|o| o == &cur) {
                                            let next = if forward {
                                                (pos + 1) % options.len()
                                            } else {
                                                (pos + options.len() - 1) % options.len()
                                            };
                                            if self
                                                .settings
                                                .set_value_str(def.key, &options[next])
                                                .is_ok()
                                            {
                                                let _ = self.settings.save();
                                            }
                                        }
                                    }
                                }
                                SettingType::Integer { .. } | SettingType::StringVal => {
                                    if matches!(key, "Return") {
                                        self.settings_editing = Some(idx);
                                        self.settings_edit_buf =
                                            self.settings.get_value_str(def.key);
                                    }
                                }
                                SettingType::BufferEditor => {
                                    if matches!(key, "Return" | "Space" | "l" | "Right") {
                                        match def.key {
                                            "keymaps" => self.open_keymaps_editor(),
                                            "extension_registries" => self.open_registries_editor(),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        SettingsRow::ExtCategory(name) => {
                            if matches!(key, "Tab" | "Return" | "Space") {
                                let collapsed = self
                                    .ext_settings_collapsed
                                    .entry(name.clone())
                                    .or_insert(false);
                                *collapsed = !*collapsed;
                            }
                        }
                        SettingsRow::ExtSetting(ext_name, ext_key) => {
                            let ext_name = ext_name.clone();
                            let ext_key = ext_key.clone();
                            if let Some(def) = self.find_ext_setting_def(&ext_name, &ext_key) {
                                match def.r#type.as_str() {
                                    "bool" => {
                                        if matches!(key, "Return" | "Space") {
                                            let cur = self.get_ext_setting(&ext_name, &ext_key);
                                            let new_val =
                                                if cur == "true" { "false" } else { "true" };
                                            self.set_ext_setting(&ext_name, &ext_key, new_val);
                                        }
                                    }
                                    "enum" => {
                                        let forward =
                                            matches!(key, "Return" | "Space" | "l" | "Right");
                                        let backward = matches!(key, "h" | "Left");
                                        if (forward || backward) && !def.options.is_empty() {
                                            let cur = self.get_ext_setting(&ext_name, &ext_key);
                                            if let Some(pos) =
                                                def.options.iter().position(|o| o == &cur)
                                            {
                                                let next = if forward {
                                                    (pos + 1) % def.options.len()
                                                } else {
                                                    (pos + def.options.len() - 1)
                                                        % def.options.len()
                                                };
                                                self.set_ext_setting(
                                                    &ext_name,
                                                    &ext_key,
                                                    &def.options[next],
                                                );
                                            }
                                        }
                                    }
                                    _ => {
                                        if matches!(key, "Return") {
                                            self.settings_edit_buf =
                                                self.get_ext_setting(&ext_name, &ext_key);
                                            self.ext_settings_editing = Some((ext_name, ext_key));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Paste clipboard text into the active settings input (search query or inline edit buffer).
    pub fn settings_paste(&mut self, text: &str) {
        // Strip newlines — settings values are single-line.
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if self.settings_input_active {
            self.settings_query.push_str(&clean);
            self.settings_selected = 0;
            self.settings_scroll_top = 0;
        } else if self.settings_editing.is_some() {
            self.settings_edit_buf.push_str(&clean);
        }
    }

    /// Open a scratch buffer for editing user keymaps (one per line).
    pub fn open_keymaps_editor(&mut self) {
        // If a keymaps buffer already exists, switch to it
        let existing_buf_id = self
            .buffer_manager
            .iter()
            .find(|(_, state)| state.is_keymaps_buf)
            .map(|(id, _)| *id);

        if let Some(buf_id) = existing_buf_id {
            // Find a tab showing this buffer
            let tab_idx = self
                .active_group()
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| {
                    self.windows
                        .get(&tab.active_window)
                        .is_some_and(|w| w.buffer_id == buf_id)
                })
                .map(|(i, _)| i);

            if let Some(idx) = tab_idx {
                self.active_group_mut().active_tab = idx;
            } else {
                // Buffer exists but not shown — point current window at it
                self.active_window_mut().buffer_id = buf_id;
                self.view_mut().cursor.line = 0;
                self.view_mut().cursor.col = 0;
            }
            self.settings_has_focus = false;
            return;
        }

        // Build content: header comment + one keymap per line
        let mut content = String::from(
            "# User keymaps — one per line.  :w to save.\n\
             # Format: mode keys :command\n\
             # Modes: n (normal), v (visual), i (insert), c (command)\n\
             # Keys:  single char (x), modifier (<C-x>, <A-x>), sequence (gcc)\n\
             #\n\
             # In VSCode mode, \"n\" keymaps apply (use modifiers like <C-x>, <A-x>).\n\
             # Run :Keybindings to see all built-in keybindings and command names.\n\
             #\n\
             # Examples:\n\
             # n <C-/> :Commentary\n\
             # v <C-/> :Commentary\n\
             # n gcc   :Commentary\n\
             # n <A-j> :move +1\n\
             # n <A-k> :move -1\n\
             #\n",
        );
        for km in &self.settings.keymaps {
            content.push_str(km);
            content.push('\n');
        }
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.is_keymaps_buf = true;
            state.dirty = false;
        }

        // Open in a new tab (same pattern as open_file_in_tab)
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.settings_has_focus = false;
        self.message = "Edit keymaps (one per line: mode keys :command). :w to save.".to_string();
    }

    /// Save keymaps buffer content back to settings.
    pub fn save_keymaps_buffer(&mut self) -> Result<(), String> {
        let state = self.active_buffer_state();
        let rope = &state.buffer.content;
        let mut keymaps = Vec::new();
        for line_idx in 0..rope.len_lines() {
            let line: String = rope.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Validate the keymap definition
            if parse_keymap_def(trimmed).is_none() {
                return Err(format!(
                    "Invalid keymap on line {}: \"{}\" (expected: mode keys :command)",
                    line_idx + 1,
                    trimmed
                ));
            }
            keymaps.push(trimmed.to_string());
        }

        self.settings.keymaps = keymaps;
        self.rebuild_user_keymaps();
        let _ = self.settings.save();
        let count = self.settings.keymaps.len();
        self.active_buffer_state_mut().dirty = false;
        self.message = format!(
            "{} keymap{} saved to settings",
            count,
            if count == 1 { "" } else { "s" }
        );
        Ok(())
    }

    /// Open a command-line window (`q:` for commands, `q/`/`q?` for searches).
    /// Shows history in a scratch buffer. Enter on a line executes it.
    pub fn open_cmdline_window(&mut self, is_search: bool) {
        let history = if is_search {
            &self.history.search_history
        } else {
            &self.history.command_history
        };

        // Build content: one history entry per line, empty line at end for new entry
        let mut content = String::new();
        for entry in history.iter() {
            content.push_str(entry);
            content.push('\n');
        }
        content.push('\n'); // empty line at bottom for new command

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.is_cmdline_buf = true;
            state.cmdline_is_search = is_search;
            state.dirty = false;
            state.scratch_name = Some(if is_search {
                "[Search History]".to_string()
            } else {
                "[Command History]".to_string()
            });
        }

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        // Move cursor to last line (the empty line for new entry)
        let total = self.buffer().len_lines();
        self.view_mut().cursor.line = total.saturating_sub(1);
        self.view_mut().cursor.col = 0;

        self.mode = Mode::Normal;
        self.message = "Press Enter to execute, q to close".to_string();
    }

    /// Execute the current line in a command-line window buffer.
    /// Called when Enter is pressed in a cmdline buffer in Normal mode.
    pub fn cmdline_window_execute(&mut self) -> EngineAction {
        let is_search = self.active_buffer_state().cmdline_is_search;
        let line_idx = self.view().cursor.line;
        let line: String = self
            .buffer()
            .content
            .line(line_idx)
            .chars()
            .collect::<String>()
            .trim()
            .to_string();

        if line.is_empty() {
            return EngineAction::None;
        }

        // Close the cmdline window
        self.close_tab();

        if is_search {
            // Execute as a forward search
            self.search_query = line;
            self.search_direction = SearchDirection::Forward;
            self.run_search();
            self.search_next();
        } else {
            // Execute as an ex command
            return self.execute_command(&line);
        }
        EngineAction::None
    }

    /// Open a read-only reference buffer listing all default keybindings.
    /// `force_vscode`: `None` = auto-detect from current mode,
    /// `Some(true)` = VSCode, `Some(false)` = Vim.
    pub fn open_keybindings_reference_for(&mut self, force_vscode: Option<bool>) {
        let is_vscode = force_vscode.unwrap_or_else(|| self.is_vscode_mode());
        let scratch_name = if is_vscode {
            "Keybindings (VSCode)"
        } else {
            "Keybindings (Vim)"
        };

        // Reuse existing buffer for the same mode if already open
        let existing_buf_id = self
            .buffer_manager
            .iter()
            .find(|(_, state)| state.scratch_name.as_deref() == Some(scratch_name))
            .map(|(id, _)| *id);

        if let Some(buf_id) = existing_buf_id {
            let tab_idx = self
                .active_group()
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| {
                    self.windows
                        .get(&tab.active_window)
                        .is_some_and(|w| w.buffer_id == buf_id)
                })
                .map(|(i, _)| i);
            if let Some(idx) = tab_idx {
                self.active_group_mut().active_tab = idx;
            } else {
                self.active_window_mut().buffer_id = buf_id;
                self.view_mut().cursor.line = 0;
                self.view_mut().cursor.col = 0;
            }
            return;
        }

        let content = if is_vscode {
            keybindings_reference_vscode()
        } else {
            keybindings_reference_vim()
        };

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.scratch_name = Some(scratch_name.to_string());
            state.read_only = true;
            state.dirty = false;
        }

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        let mode_name = if is_vscode { "VSCode" } else { "Vim" };
        self.message = format!(
            "{mode_name} keybindings reference — use / to search. Try :Keybindings {}",
            if is_vscode { "vim" } else { "vscode" }
        );
    }

    /// Open a scratch buffer for editing extension registry URLs (one per line).
    pub fn open_registries_editor(&mut self) {
        // If a registries buffer already exists, switch to it
        let existing_buf_id = self
            .buffer_manager
            .iter()
            .find(|(_, state)| state.is_registries_buf)
            .map(|(id, _)| *id);

        if let Some(buf_id) = existing_buf_id {
            let tab_idx = self
                .active_group()
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| {
                    self.windows
                        .get(&tab.active_window)
                        .is_some_and(|w| w.buffer_id == buf_id)
                })
                .map(|(i, _)| i);

            if let Some(idx) = tab_idx {
                self.active_group_mut().active_tab = idx;
            } else {
                self.active_window_mut().buffer_id = buf_id;
                self.view_mut().cursor.line = 0;
                self.view_mut().cursor.col = 0;
            }
            self.settings_has_focus = false;
            return;
        }

        // Build content: header comment + one URL per line
        let mut content = String::from(
            "# Extension registries — one URL per line.\n\
             # Lines starting with # are comments.\n",
        );
        for url in &self.settings.extension_registries {
            content.push_str(url);
            content.push('\n');
        }

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.is_registries_buf = true;
            state.dirty = false;
        }

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.settings_has_focus = false;
        self.message =
            "Edit extension registries (one URL per line, # comments). :w to save.".to_string();
    }

    /// Save registries buffer content back to settings.
    pub fn save_registries_buffer(&mut self) -> Result<(), String> {
        let state = self.active_buffer_state();
        let rope = &state.buffer.content;
        let mut urls = Vec::new();
        for line_idx in 0..rope.len_lines() {
            let line: String = rope.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
                return Err(format!(
                    "Invalid URL on line {}: \"{}\" (must start with http:// or https://)",
                    line_idx + 1,
                    trimmed
                ));
            }
            urls.push(trimmed.to_string());
        }

        self.settings.extension_registries = urls;
        let _ = self.settings.save();
        let count = self.settings.extension_registries.len();
        self.active_buffer_state_mut().dirty = false;
        self.message = format!(
            "{} registr{} saved to settings",
            count,
            if count == 1 { "y" } else { "ies" }
        );
        Ok(())
    }

    // ── AI assistant panel ─────────────────────────────────────────────────────

    /// Send the current `ai_input` as a user message; clears input and spawns background thread.
    pub fn ai_send_message(&mut self) {
        let text = self.ai_input.trim().to_string();
        if text.is_empty() || self.ai_streaming {
            return;
        }
        self.ai_messages.push(AiMessage {
            role: "user".to_string(),
            content: text,
        });
        self.ai_input.clear();
        self.ai_input_cursor = 0;
        self.ai_streaming = true;

        let provider = self.settings.ai_provider.clone();
        let api_key = self.settings.ai_api_key.clone();
        let base_url = self.settings.ai_base_url.clone();
        let model = self.settings.ai_model.clone();
        let messages = self.ai_messages.clone();
        let system = String::new();

        let (tx, rx) = std::sync::mpsc::channel();
        self.ai_rx = Some(rx);

        std::thread::spawn(move || {
            let result =
                super::ai::send_chat(&provider, &api_key, &base_url, &model, &messages, &system);
            let _ = tx.send(result);
        });

        // Scroll to bottom so the user sees the new message
        self.ai_scroll_top = self.ai_messages.len().saturating_sub(1);
    }

    /// Non-blocking poll for a completed AI response. Returns `true` if something changed.
    pub fn poll_ai(&mut self) -> bool {
        let result = if let Some(rx) = &self.ai_rx {
            rx.try_recv().ok()
        } else {
            return false;
        };
        let Some(res) = result else {
            return false;
        };
        self.ai_rx = None;
        self.ai_streaming = false;
        match res {
            Ok(reply) => {
                self.ai_messages.push(AiMessage {
                    role: "assistant".to_string(),
                    content: reply,
                });
                self.ai_scroll_top = self.ai_messages.len().saturating_sub(1);
            }
            Err(e) => {
                self.message = format!("AI error: {e}");
            }
        }
        true
    }

    /// Clear the AI conversation history and cancel any in-flight request.
    pub fn ai_clear(&mut self) {
        self.ai_messages.clear();
        self.ai_rx = None;
        self.ai_streaming = false;
        self.ai_scroll_top = 0;
        self.message = "AI conversation cleared.".to_string();
    }

    /// Handle keyboard input for the AI sidebar panel.
    /// Returns `true` if the key was consumed.
    /// Insert text at the current ai_input cursor position (used for paste).
    pub fn ai_insert_text(&mut self, text: &str) {
        let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
        self.ai_input.insert_str(byte, text);
        self.ai_input_cursor += text.chars().count();
    }

    pub fn handle_ai_panel_key(&mut self, key: &str, ctrl: bool, unicode: Option<char>) -> bool {
        if self.ai_input_active {
            let char_len = self.ai_input.chars().count();
            match key {
                "Escape" => {
                    self.ai_input_active = false;
                }
                "Return" if !ctrl => {
                    self.ai_send_message();
                    self.ai_input_active = false;
                }
                "BackSpace" => {
                    if self.ai_input_cursor > 0 {
                        self.ai_input_cursor -= 1;
                        let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                        let next = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor + 1);
                        self.ai_input.drain(byte..next);
                    }
                }
                "Delete" => {
                    if self.ai_input_cursor < char_len {
                        let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                        let next = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor + 1);
                        self.ai_input.drain(byte..next);
                    }
                }
                "Left" => {
                    self.ai_input_cursor = self.ai_input_cursor.saturating_sub(1);
                }
                "Right" => {
                    self.ai_input_cursor = (self.ai_input_cursor + 1).min(char_len);
                }
                "Home" => {
                    self.ai_input_cursor = 0;
                }
                "End" => {
                    self.ai_input_cursor = char_len;
                }
                _ if ctrl && key == "a" => {
                    self.ai_input_cursor = 0;
                }
                _ if ctrl && key == "e" => {
                    self.ai_input_cursor = char_len;
                }
                _ if ctrl && key == "k" => {
                    let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                    self.ai_input.truncate(byte);
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                            self.ai_input.insert(byte, ch);
                            self.ai_input_cursor += 1;
                        }
                    }
                }
            }
            return true;
        }

        match key {
            "q" | "Escape" => {
                self.ai_has_focus = false;
                true
            }
            "i" | "a" | "Return" => {
                self.ai_input_active = true;
                true
            }
            "j" | "Down" => {
                self.ai_scroll_top = self.ai_scroll_top.saturating_add(1);
                true
            }
            "k" | "Up" => {
                self.ai_scroll_top = self.ai_scroll_top.saturating_sub(1);
                true
            }
            "G" => {
                self.ai_scroll_top = self.ai_messages.len().saturating_sub(1);
                true
            }
            "g" => {
                self.ai_scroll_top = 0;
                true
            }
            "c" if ctrl => {
                // Ctrl-C: clear conversation
                self.ai_clear();
                true
            }
            _ => false,
        }
    }

    // ── AI inline completions (ghost text) ───────────────────────────────────

    /// Clear any visible ghost text and cancel the pending completion timer.
    pub fn ai_ghost_clear(&mut self) {
        self.ai_ghost_text = None;
        self.ai_ghost_alternatives.clear();
        self.ai_ghost_alt_idx = 0;
        self.ai_completion_ticks = None;
        // Don't close the rx; the background thread may still send — we'll
        // just ignore the result since ai_completion_rx is checked only when
        // ticks fires.
    }

    /// Reset the debounce counter. Called after every insert-mode keystroke
    /// when `settings.ai_completions` is enabled.
    pub fn ai_completion_reset_timer(&mut self) {
        // Clear any stale ghost text; the counter will fire a new request.
        self.ai_ghost_text = None;
        self.ai_ghost_alternatives.clear();
        self.ai_ghost_alt_idx = 0;
        // ~15 ticks ≈ 250 ms at 60 fps; backends decrement each frame.
        self.ai_completion_ticks = Some(15);
    }

    /// Called by the backend each frame. Decrements the tick counter and
    /// fires a completion request when it reaches zero. Returns `true` if
    /// a redraw is needed.
    pub fn tick_ai_completion(&mut self) -> bool {
        // First, check if a background completion has arrived.
        let mut redraw = false;
        if let Some(rx) = &self.ai_completion_rx {
            if let Ok(result) = rx.try_recv() {
                self.ai_completion_rx = None;
                match result {
                    Ok(mut alternatives) => {
                        if !alternatives.is_empty() {
                            // Strip any leading characters that the AI repeated from the
                            // prefix (e.g. the model returns `"PlayerObject":` when the
                            // buffer already ends with `"` before the cursor).
                            // We check overlaps up to 16 chars and strip the longest match.
                            let tail = std::mem::take(&mut self.ai_completion_prefix_tail);
                            for alt in &mut alternatives {
                                let max_n = alt.chars().count().min(tail.chars().count()).min(16);
                                let overlap_bytes = (1..=max_n).rev().find_map(|n| {
                                    let alt_prefix: String = alt.chars().take(n).collect();
                                    if tail.ends_with(alt_prefix.as_str()) {
                                        Some(alt_prefix.len()) // String::len() = byte length
                                    } else {
                                        None
                                    }
                                });
                                if let Some(b) = overlap_bytes {
                                    *alt = alt[b..].to_string();
                                }
                            }
                            alternatives.retain(|a| !a.is_empty());
                        }
                        if !alternatives.is_empty() {
                            self.ai_ghost_alternatives = alternatives;
                            self.ai_ghost_alt_idx = 0;
                            self.ai_ghost_text = Some(self.ai_ghost_alternatives[0].clone());
                            redraw = true;
                        }
                    }
                    Err(_) => {
                        // Silently ignore errors for inline completions.
                    }
                }
            }
        }

        // Decrement the countdown and fire when it hits zero.
        if let Some(ticks) = self.ai_completion_ticks {
            if ticks == 0 {
                self.ai_completion_ticks = None;
                self.ai_fire_completion_request();
            } else {
                self.ai_completion_ticks = Some(ticks - 1);
            }
        }

        redraw
    }

    /// Spawn a background thread to request a ghost-text completion.
    fn ai_fire_completion_request(&mut self) {
        if !self.settings.ai_completions {
            return;
        }
        // Only trigger in Insert mode.
        if self.mode != Mode::Insert {
            return;
        }

        // Build prefix: all text in the active buffer up to the cursor.
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let cursor_char = line_start + col;
        let total_chars = self.buffer().content.len_chars();

        // Limit prefix to last ~2000 chars to keep latency reasonable.
        let prefix_start = cursor_char.saturating_sub(2000);
        let prefix: String = self
            .buffer()
            .content
            .slice(prefix_start..cursor_char)
            .chars()
            .collect();

        // Suffix: text after the cursor on the same line (for FIM models).
        let line_end = self
            .buffer()
            .content
            .slice(..)
            .chars()
            .enumerate()
            .skip(cursor_char)
            .find(|&(_, c)| c == '\n')
            .map(|(i, _)| i)
            .unwrap_or(total_chars);
        let suffix: String = self
            .buffer()
            .content
            .slice(cursor_char..line_end)
            .chars()
            .collect();

        let provider = self.settings.ai_provider.clone();
        let api_key = self.settings.ai_api_key.clone();
        let base_url = self.settings.ai_base_url.clone();
        let model = self.settings.ai_model.clone();

        // Store the last 64 chars of the prefix so tick_ai_completion can detect
        // and strip overlap when the AI repeats characters already in the buffer.
        self.ai_completion_prefix_tail = prefix
            .chars()
            .rev()
            .take(64)
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        let (tx, rx) = std::sync::mpsc::channel();
        self.ai_completion_rx = Some(rx);

        std::thread::spawn(move || {
            let result = super::ai::complete(
                &provider, &api_key, &base_url, &model, &prefix, &suffix,
            )
            .map(|text| {
                // Trim leading/trailing whitespace that many models add.
                let trimmed = text.trim_end_matches('\n').to_string();
                // Return a single alternative for now.
                vec![trimmed]
            });
            let _ = tx.send(result);
        });
    }

    // ── Swap file crash recovery ───────────────────────────────────────────

    /// Create a swap file for the given buffer.
    fn swap_create_for_buffer(&self, buf_id: BufferId) {
        if !self.settings.swap_file {
            return;
        }
        let state = match self.buffer_manager.get(buf_id) {
            Some(s) => s,
            None => return,
        };
        // Don't create swaps for preview buffers — they're temporary.
        if state.preview {
            return;
        }
        let canonical = match &state.canonical_path {
            Some(p) => p,
            None => return,
        };
        let swap_path = super::swap::swap_path_for(canonical);
        let header = super::swap::SwapHeader {
            file_path: canonical.clone(),
            pid: std::process::id(),
            modified: super::swap::now_iso8601(),
        };
        let content = state.buffer.to_string();
        super::swap::write_swap(&swap_path, &header, &content);
    }

    /// Check for a stale swap file when opening a file.
    /// Returns `true` if a recovery dialog is now pending (caller should stop).
    fn swap_check_on_open(&mut self, buf_id: BufferId) -> bool {
        if !self.settings.swap_file {
            return false;
        }
        // Don't overwrite an existing recovery dialog.
        if self.pending_swap_recovery.is_some() {
            self.swap_create_for_buffer(buf_id);
            return false;
        }
        let (canonical, file_path) = {
            let state = match self.buffer_manager.get(buf_id) {
                Some(s) => s,
                None => return false,
            };
            // Don't create swaps for preview buffers.
            if state.preview {
                return false;
            }
            let canonical = match &state.canonical_path {
                Some(p) => p.clone(),
                None => return false,
            };
            let file_path = match &state.file_path {
                Some(p) => p.clone(),
                None => return false,
            };
            (canonical, file_path)
        };
        let swap_path = super::swap::swap_path_for(&canonical);
        if !swap_path.exists() {
            // No swap file — create a fresh one.
            self.swap_create_for_buffer(buf_id);
            return false;
        }
        // Swap file exists — parse it.
        let (header, content) = match super::swap::read_swap(&swap_path) {
            Some(pair) => pair,
            None => {
                // Malformed swap file — delete and create fresh.
                super::swap::delete_swap(&swap_path);
                self.swap_create_for_buffer(buf_id);
                return false;
            }
        };
        if super::swap::is_pid_alive(header.pid) {
            if header.pid == std::process::id() {
                // Same process re-opening the file — just update the swap.
                self.swap_create_for_buffer(buf_id);
                return false;
            }
            // Another live process is editing this file.
            let fname = file_path.file_name().unwrap_or_default().to_string_lossy();
            self.message = format!(
                "W: \"{}\" is being edited by PID {} — opening read-only copy",
                fname, header.pid
            );
            return false;
        }
        // PID is dead → offer recovery via dialog.
        let fname = file_path.file_name().unwrap_or_default().to_string_lossy();
        self.pending_swap_recovery = Some(SwapRecovery {
            swap_path,
            recovered_content: content,
            buffer_id: buf_id,
        });
        self.show_dialog(
            "swap_recovery",
            "Swap File Found",
            vec![
                format!("A swap file was found for \"{}\".", fname),
                format!("Modified: {}", header.modified),
                format!("Original PID: {} (no longer running)", header.pid),
            ],
            vec![
                DialogButton {
                    label: "Recover".into(),
                    hotkey: 'r',
                    action: "recover".into(),
                },
                DialogButton {
                    label: "Delete swap".into(),
                    hotkey: 'd',
                    action: "delete".into(),
                },
                DialogButton {
                    label: "Abort".into(),
                    hotkey: 'a',
                    action: "abort".into(),
                },
            ],
        );
        true
    }

    /// Process the result of a swap recovery dialog action.
    fn process_swap_dialog_action(&mut self, action: &str) -> EngineAction {
        let recovery = match self.pending_swap_recovery.take() {
            Some(r) => r,
            None => return EngineAction::None,
        };
        match action {
            "recover" => {
                let state = self.buffer_manager.get_mut(recovery.buffer_id);
                if let Some(state) = state {
                    let len = state.buffer.len_chars();
                    state.buffer.delete_range(0, len);
                    if !recovery.recovered_content.is_empty() {
                        state.buffer.insert(0, &recovery.recovered_content);
                    }
                    state.dirty = true;
                }
                super::swap::delete_swap(&recovery.swap_path);
                self.swap_create_for_buffer(recovery.buffer_id);
                self.message = "Recovered from swap file".to_string();
            }
            "delete" => {
                super::swap::delete_swap(&recovery.swap_path);
                self.swap_create_for_buffer(recovery.buffer_id);
                self.message = "Swap file deleted".to_string();
            }
            "abort" | "cancel" => {
                super::swap::delete_swap(&recovery.swap_path);
                self.close_tab();
                self.message.clear();
            }
            _ => {}
        }
        EngineAction::None
    }

    // ─── Dialog system ─────────────────────────────────────────────────

    /// Show a modal dialog.
    pub fn show_dialog(
        &mut self,
        tag: &str,
        title: &str,
        body: Vec<String>,
        buttons: Vec<DialogButton>,
    ) {
        self.dialog = Some(Dialog {
            title: title.to_string(),
            body,
            buttons,
            selected: 0,
            tag: tag.to_string(),
            input: None,
        });
    }

    /// Convenience: show an error dialog with a single OK button.
    #[allow(dead_code)]
    pub fn show_error_dialog(&mut self, title: &str, message: &str) {
        self.show_dialog(
            "error",
            title,
            vec![message.to_string()],
            vec![DialogButton {
                label: "OK".into(),
                hotkey: 'o',
                action: "ok".into(),
            }],
        );
    }

    /// Click a dialog button by index.  Returns the `EngineAction` from
    /// processing the dialog result, or `None` if the index is out of range.
    pub fn dialog_click_button(&mut self, idx: usize) -> EngineAction {
        let (tag, action, input_value) = {
            let dialog = match self.dialog.as_ref() {
                Some(d) => d,
                None => return EngineAction::None,
            };
            let btn = match dialog.buttons.get(idx) {
                Some(b) => b,
                None => return EngineAction::None,
            };
            let iv = dialog.input.as_ref().map(|i| i.value.clone());
            (dialog.tag.clone(), btn.action.clone(), iv)
        };
        self.dialog = None;
        self.process_dialog_result(&tag, &action, input_value.as_deref())
    }

    /// Handle a key press when a dialog is open.
    /// Returns `Some((tag, action))` when the dialog is dismissed, `None` to keep it open.
    fn handle_dialog_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
    ) -> Option<(String, String)> {
        let dialog = self.dialog.as_mut()?;
        let has_input = dialog.input.is_some();

        // TUI sends key_name="" with the char in unicode; GTK sends key_name="r".
        let effective = if !key_name.is_empty() {
            key_name.to_string()
        } else {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        };

        match effective.as_str() {
            "Escape" => {
                let tag = dialog.tag.clone();
                self.dialog = None;
                Some((tag, "cancel".to_string()))
            }
            "Return" => {
                let tag = dialog.tag.clone();
                let action = dialog
                    .buttons
                    .get(dialog.selected)
                    .map(|b| b.action.clone())
                    .unwrap_or_else(|| "cancel".to_string());
                self.dialog = None;
                Some((tag, action))
            }
            "BackSpace" if has_input => {
                if let Some(ref mut input) = dialog.input {
                    input.value.pop();
                }
                None
            }
            "Tab" | "Shift_Tab" => {
                let len = dialog.buttons.len();
                if len > 0 {
                    if effective == "Shift_Tab" {
                        dialog.selected = if dialog.selected > 0 {
                            dialog.selected - 1
                        } else {
                            len - 1
                        };
                    } else {
                        dialog.selected = (dialog.selected + 1) % len;
                    }
                }
                None
            }
            "Left" | "h" | "Up" | "k" if !has_input => {
                let len = dialog.buttons.len();
                if len > 0 {
                    dialog.selected = if dialog.selected > 0 {
                        dialog.selected - 1
                    } else {
                        len - 1
                    };
                }
                None
            }
            "Right" | "l" | "Down" | "j" if !has_input => {
                let len = dialog.buttons.len();
                if len > 0 {
                    dialog.selected = (dialog.selected + 1) % len;
                }
                None
            }
            _ => {
                // When dialog has a text input, printable chars go there.
                if has_input {
                    if let Some(ch) = unicode {
                        if let Some(ref mut input) = dialog.input {
                            input.value.push(ch);
                        }
                    }
                    return None;
                }
                // Check hotkeys (case-insensitive).
                let ch = effective
                    .chars()
                    .next()
                    .unwrap_or('\0')
                    .to_ascii_lowercase();
                for btn in &dialog.buttons {
                    if btn.hotkey == ch {
                        let tag = dialog.tag.clone();
                        let action = btn.action.clone();
                        self.dialog = None;
                        return Some((tag, action));
                    }
                }
                None
            }
        }
    }

    /// Dispatch a dialog result to the appropriate handler.
    fn process_dialog_result(
        &mut self,
        tag: &str,
        action: &str,
        input_value: Option<&str>,
    ) -> EngineAction {
        match tag {
            "swap_recovery" => self.process_swap_dialog_action(action),
            "confirm_move" => {
                if action == "yes" {
                    if let Some((src, dest)) = self.pending_move.take() {
                        match self.move_file(&src, &dest) {
                            Ok(()) => {
                                let name = src
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                self.message = format!("Moved '{}' to '{}'", name, dest.display());
                                self.explorer_needs_refresh = true;
                            }
                            Err(e) => {
                                self.message = e;
                            }
                        }
                    }
                } else {
                    self.pending_move = None;
                }
                EngineAction::None
            }
            "ext_remove" => {
                if let Some(name) = self.pending_ext_remove.take() {
                    match action {
                        "remove" | "keep_tools" => self.ext_remove(&name, false),
                        "remove_all" => self.ext_remove(&name, true),
                        _ => {} // cancel — do nothing
                    }
                }
                EngineAction::None
            }
            "ssh_passphrase" => {
                if action == "ok" {
                    let passphrase = input_value.unwrap_or("");
                    if let Some(op) = self.pending_git_remote_op.take() {
                        let dir =
                            git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
                        let result = match op.as_str() {
                            "push" => git::push_with_passphrase(&dir, passphrase),
                            "pull" => git::pull_with_passphrase(&dir, passphrase),
                            "fetch" => git::fetch_with_passphrase(&dir, passphrase),
                            _ => Err(format!("unknown git op: {}", op)),
                        };
                        match result {
                            Ok(msg) => {
                                let default_msg = match op.as_str() {
                                    "push" => "Pushed.",
                                    "pull" => "Already up to date.",
                                    "fetch" => "Fetched.",
                                    _ => "Done.",
                                };
                                self.message = if msg.is_empty() {
                                    default_msg.to_string()
                                } else {
                                    msg
                                };
                            }
                            Err(e) => self.message = format!("{}: {}", op, e),
                        }
                        self.sc_refresh();
                    }
                } else {
                    self.pending_git_remote_op = None;
                }
                EngineAction::None
            }
            tag if tag.starts_with("open_ext_url:") => {
                // Extension-provided link — user confirmed "Open".
                if action == "open" {
                    let url = &tag["open_ext_url:".len()..];
                    if is_safe_url(url) {
                        return EngineAction::OpenUrl(url.to_string());
                    }
                }
                EngineAction::None
            }
            "code_actions" => {
                if let Some(idx_str) = action.strip_prefix("apply_") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Some(ca) = self.pending_code_action_choices.get(idx).cloned() {
                            self.pending_code_action_choices.clear();
                            if let Some(edit) = ca.edit {
                                self.apply_workspace_edit(edit);
                                self.message = format!("Applied: {}", ca.title);
                            } else {
                                self.message = format!("No edit available for '{}'", ca.title);
                            }
                        }
                    }
                } else {
                    self.pending_code_action_choices.clear();
                }
                EngineAction::None
            }
            _ => EngineAction::None,
        }
    }

    /// Mark the active buffer as needing a swap write.
    pub fn swap_mark_dirty(&mut self) {
        if self.settings.swap_file {
            let id = self.active_buffer_id();
            self.swap_write_needed.insert(id);
        }
    }

    /// Periodically write swap files for dirty buffers.
    /// Called from both GTK and TUI event loops (~20 Hz).  The method only
    /// does real work when `updatetime` milliseconds have elapsed.
    pub fn tick_swap_files(&mut self) {
        if !self.settings.swap_file || self.swap_write_needed.is_empty() {
            return;
        }
        let elapsed = self.swap_last_write.elapsed().as_millis() as u32;
        if elapsed < self.settings.updatetime {
            return;
        }
        let buf_ids: Vec<BufferId> = self.swap_write_needed.drain().collect();
        for buf_id in buf_ids {
            self.swap_create_for_buffer(buf_id);
        }
        self.swap_last_write = std::time::Instant::now();
    }

    /// Delete the swap file for a single buffer.
    fn swap_delete_for_buffer(&self, buf_id: BufferId) {
        let state = match self.buffer_manager.get(buf_id) {
            Some(s) => s,
            None => return,
        };
        let canonical = match &state.canonical_path {
            Some(p) => p,
            None => return,
        };
        let swap_path = super::swap::swap_path_for(canonical);
        super::swap::delete_swap(&swap_path);
    }

    /// Delete swap files for ALL open buffers.  Called on clean shutdown.
    pub fn cleanup_all_swaps(&self) {
        for buf_id in self.buffer_manager.list() {
            self.swap_delete_for_buffer(buf_id);
        }
    }

    /// Check all open buffers for stale swap files.
    /// Called after session restore to catch any crashed sessions.
    /// Only the first stale swap triggers a recovery dialog — the rest
    /// get fresh swap files created silently.
    pub fn swap_check_all_buffers(&mut self) {
        if !self.settings.swap_file {
            return;
        }
        let buf_ids = self.buffer_manager.list();
        for buf_id in buf_ids {
            if self.pending_swap_recovery.is_some() {
                // Already showing a recovery dialog — just create swaps for the rest.
                self.swap_create_for_buffer(buf_id);
            } else {
                self.swap_check_on_open(buf_id);
            }
        }
        // Also scan the swap directory for orphaned swaps (files that
        // aren't in the restored session).
        self.swap_scan_stale();
    }

    /// Scan the swap directory for stale swap files with dead PIDs that
    /// don't correspond to any currently-open buffer.  Opens the first
    /// orphaned file in a new tab and offers recovery.
    fn swap_scan_stale(&mut self) {
        if !self.settings.swap_file || self.pending_swap_recovery.is_some() {
            return;
        }
        let stale = super::swap::find_stale_swaps();
        // Collect canonical paths of all currently-open buffers.
        let open_paths: std::collections::HashSet<PathBuf> = self
            .buffer_manager
            .list()
            .into_iter()
            .filter_map(|id| {
                self.buffer_manager
                    .get(id)
                    .and_then(|s| s.canonical_path.clone())
            })
            .collect();
        for (header, swap_path) in stale {
            if open_paths.contains(&header.file_path) {
                // Already handled by swap_check_all_buffers above.
                continue;
            }
            // The file from this stale swap isn't open — open it and offer recovery.
            if !header.file_path.exists() {
                // Original file was deleted — clean up the orphaned swap.
                super::swap::delete_swap(&swap_path);
                continue;
            }
            // Open the file in a new tab.  `open_file_in_tab` calls
            // `swap_check_on_open` internally, which will detect the
            // stale swap and set `pending_swap_recovery` for us.
            self.open_file_in_tab(&header.file_path);
            if self.pending_swap_recovery.is_some() {
                return;
            }
        }
    }

    /// Accept the current ghost text by inserting it at the cursor.
    pub fn ai_accept_ghost(&mut self) {
        if let Some(ghost) = self.ai_ghost_text.take() {
            if !ghost.is_empty() {
                let line = self.view().cursor.line;
                let col = self.view().cursor.col;
                let char_idx = self.buffer().line_to_char(line) + col;
                let char_count = ghost.chars().count();
                self.insert_with_undo(char_idx, &ghost);
                self.view_mut().cursor.col += char_count;
            }
        }
        self.ai_ghost_clear();
    }

    /// Show the next ghost text alternative (Alt+]).
    pub fn ai_ghost_next_alt(&mut self) {
        if self.ai_ghost_alternatives.is_empty() {
            return;
        }
        self.ai_ghost_alt_idx = (self.ai_ghost_alt_idx + 1) % self.ai_ghost_alternatives.len();
        self.ai_ghost_text = Some(self.ai_ghost_alternatives[self.ai_ghost_alt_idx].clone());
    }

    /// Show the previous ghost text alternative (Alt+[).
    pub fn ai_ghost_prev_alt(&mut self) {
        if self.ai_ghost_alternatives.is_empty() {
            return;
        }
        let len = self.ai_ghost_alternatives.len();
        self.ai_ghost_alt_idx = (self.ai_ghost_alt_idx + len - 1) % len;
        self.ai_ghost_text = Some(self.ai_ghost_alternatives[self.ai_ghost_alt_idx].clone());
    }

    /// Re-send didOpen for all open buffers that match a given language ID.
    /// Called after a new server is detected/started mid-session.
    fn lsp_reopen_buffers_for_language(&mut self, lang_id: &str) {
        let buffers: Vec<(PathBuf, String)> = self
            .buffer_manager
            .list()
            .iter()
            .filter_map(|&bid| {
                let s = self.buffer_manager.get(bid)?;
                if s.lsp_language_id.as_deref() == Some(lang_id) {
                    let path = s.file_path.as_ref()?.clone();
                    Some((path, s.buffer.to_string()))
                } else {
                    None
                }
            })
            .collect();
        if let Some(mgr) = &mut self.lsp_manager {
            for (path, text) in buffers {
                let _ = mgr.notify_did_open(&path, &text);
            }
        }
    }

    /// Notify LSP that a file was saved.
    fn lsp_did_save(&mut self, buffer_id: BufferId) {
        if !self.settings.lsp_enabled {
            return;
        }
        let (path, text) = {
            let state = match self.buffer_manager.get(buffer_id) {
                Some(s) => s,
                None => return,
            };
            let path = match &state.file_path {
                Some(p) => p.clone(),
                None => return,
            };
            if state.lsp_language_id.is_none() {
                return;
            }
            (path, state.buffer.to_string())
        };
        if let Some(mgr) = &mut self.lsp_manager {
            mgr.notify_did_save(&path, &text);
        }
        // Also flush any pending didChange
        self.lsp_dirty_buffers.remove(&buffer_id);
    }

    /// Notify LSP that a file was closed.
    fn lsp_did_close(&mut self, buffer_id: BufferId) {
        let path = self
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.clone());
        if let Some(ref path) = path {
            if let Some(mgr) = &mut self.lsp_manager {
                mgr.notify_did_close(path);
            }
            self.lsp_diagnostics.remove(path);
        }
    }

    /// Flush any pending didChange notifications (called from UI poll loop).
    /// Request semantic tokens for a file from the LSP server.
    /// Multiple requests can be in flight simultaneously; responses are matched by request ID.
    pub fn lsp_request_semantic_tokens(&mut self, path: &Path) {
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(req_id) = mgr.request_semantic_tokens(path) {
                self.lsp_pending_semantic_tokens
                    .insert(req_id, path.to_path_buf());
            }
        }
    }

    pub fn lsp_flush_changes(&mut self) {
        if self.lsp_manager.is_none() {
            return;
        }
        let dirty: Vec<BufferId> = self.lsp_dirty_buffers.keys().copied().collect();
        for buffer_id in dirty {
            self.lsp_dirty_buffers.remove(&buffer_id);
            let (path, text) = {
                let state = match self.buffer_manager.get(buffer_id) {
                    Some(s) => s,
                    None => continue,
                };
                let path = match &state.file_path {
                    Some(p) => p.clone(),
                    None => continue,
                };
                if state.lsp_language_id.is_none() {
                    continue;
                }
                (path, state.buffer.to_string())
            };
            // Clear stale position-based data immediately — line numbers from
            // the previous buffer state would highlight/annotate wrong lines.
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.semantic_tokens.clear();
            }
            self.lsp_diagnostics.remove(&path);
            self.lsp_code_actions.remove(&path);
            self.lsp_code_action_last_line = None;
            if let Some(mgr) = &mut self.lsp_manager {
                mgr.notify_did_change(&path, &text);
            }
            // Re-request semantic tokens after the server processes the change.
            self.lsp_request_semantic_tokens(&path);
        }
    }

    /// Poll LSP for events. Called every frame from the UI event loop.
    /// Returns true if a redraw is needed.
    pub fn poll_lsp(&mut self) -> bool {
        let events = match &mut self.lsp_manager {
            Some(mgr) => mgr.poll_events(),
            None => return false,
        };
        if events.is_empty() {
            return false;
        }

        // Pre-compute canonical paths for visible buffers (once, not per-event).
        let visible_paths: Vec<PathBuf> = self
            .windows
            .values()
            .filter_map(|w| {
                self.buffer_manager
                    .get(w.buffer_id)?
                    .file_path
                    .as_ref()
                    .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
            })
            .collect();

        let mut redraw = false;
        for event in events {
            match event {
                LspEvent::Initialized(..) => {
                    // Server is ready — re-open any already-open buffers
                    let buffers: Vec<(PathBuf, String)> = self
                        .buffer_manager
                        .list()
                        .iter()
                        .filter_map(|&bid| {
                            let s = self.buffer_manager.get(bid)?;
                            let p = s.file_path.as_ref()?.clone();
                            if s.lsp_language_id.is_some() {
                                Some((p, s.buffer.to_string()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if let Some(mgr) = &mut self.lsp_manager {
                        for (path, text) in &buffers {
                            let _ = mgr.notify_did_open(path, text);
                        }
                    }
                    // Request semantic tokens for all reopened buffers.
                    for (path, _) in &buffers {
                        self.lsp_request_semantic_tokens(path);
                    }
                }
                LspEvent::Diagnostics {
                    path, diagnostics, ..
                } => {
                    // Only redraw if diagnostics affect a currently visible buffer.
                    if !redraw && visible_paths.contains(&path) {
                        redraw = true;
                    }
                    self.lsp_diagnostics.insert(path, diagnostics);
                }
                LspEvent::CompletionResponse {
                    request_id, items, ..
                } => {
                    if self.lsp_pending_completion == Some(request_id) {
                        // Popup completion response — populate display-only popup
                        self.lsp_pending_completion = None;
                        // Only show completion if still in Insert mode (or VSCode mode).
                        // The user may have pressed Escape between the request and response.
                        let in_insert = self.mode == Mode::Insert || self.is_vscode_mode();
                        if in_insert && !items.is_empty() {
                            let (cur_prefix, _) = self.completion_prefix_at_cursor();
                            let lsp_cands: Vec<String> = items
                                .iter()
                                .filter_map(|item| {
                                    let text = item.insert_text.as_deref().unwrap_or(&item.label);
                                    text.starts_with(&cur_prefix).then(|| text.to_string())
                                })
                                .collect();
                            if !lsp_cands.is_empty() {
                                self.completion_start_col =
                                    self.view().cursor.col - cur_prefix.chars().count();
                                self.completion_candidates = lsp_cands;
                                self.completion_idx = Some(0);
                                self.completion_display_only = true;
                                redraw = true;
                            }
                        }
                    }
                    // else: stale response (request already superseded) — ignore
                }
                LspEvent::DefinitionResponse { locations, .. } => {
                    self.lsp_pending_definition = None;
                    self.message.clear();
                    if let Some(loc) = locations.first() {
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        // Open the file and jump
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        // Jump to line/col
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                        redraw = true;
                    } else {
                        self.message = "No definition found".to_string();
                    }
                }
                LspEvent::HoverResponse { contents, .. } => {
                    self.lsp_pending_hover = None;
                    // Treat empty/whitespace-only hover as "no hover".
                    let text = contents.filter(|t| !t.trim().is_empty());
                    if let Some(text) = text {
                        self.lsp_hover_null_pos = None;
                        // Cancel "Loading..." auto-dismiss since we got real content.
                        self.editor_hover_dismiss_at = None;
                        if self.editor_hover.is_some() || self.editor_hover_has_focus {
                            // Popup already visible (keyboard hover or has diagnostics) — update it.
                            self.update_editor_hover_with_lsp(&text);
                        } else if let Some((line, col)) = self.lsp_hover_request_pos {
                            // Mouse hover: no popup yet — create one at the request position.
                            self.show_editor_hover(
                                line,
                                col,
                                &text,
                                EditorHoverSource::Lsp,
                                false,
                                true,
                            );
                        } else {
                            self.lsp_hover_text = Some(text);
                        }
                        redraw = true;
                    } else {
                        // LSP returned no hover — remember position to suppress re-requests.
                        if let Some(pos) = self.lsp_hover_request_pos.take() {
                            self.lsp_hover_null_pos = Some(pos);
                        }
                        if self.editor_hover.is_some()
                            && self
                                .editor_hover
                                .as_ref()
                                .is_some_and(|h| matches!(h.source, EditorHoverSource::Lsp))
                        {
                            // Dismiss "Loading..." popup (only if LSP-sourced)
                            self.dismiss_editor_hover();
                            redraw = true;
                        }
                    }
                }
                LspEvent::ServerExited {
                    server_id,
                    stderr,
                    was_initialized,
                } => {
                    let desc = self
                        .lsp_manager
                        .as_mut()
                        .map(|mgr| mgr.handle_server_exited(server_id))
                        .unwrap_or_else(|| format!("server {}", server_id));
                    if was_initialized {
                        self.message = format!("LSP {} exited", desc);
                    } else {
                        let snippet = stderr
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("no output")
                            .trim();
                        let snippet = if snippet.len() > 100 {
                            &snippet[..100]
                        } else {
                            snippet
                        };
                        self.message = format!("LSP {} failed to start: {}", desc, snippet);
                    }
                    redraw = true;
                }
                LspEvent::RegistryLookup { lang_id, .. } => {
                    // Mason registry lookups are no longer used. Ignore stale events.
                    self.lsp_lookup_in_flight.remove(&lang_id);
                }
                LspEvent::InstallComplete {
                    lang_id,
                    success,
                    output,
                } => {
                    self.lsp_installing.remove(&lang_id);
                    // preLaunchTask completion: resume debug session after build task.
                    if let Some(task_label) = lang_id.strip_prefix("dap_task:") {
                        // Append task output to Debug Output panel.
                        for line in output.lines() {
                            self.dap_output_lines.push(line.to_string());
                        }
                        if success {
                            self.dap_output_lines.push(format!(
                                "[dap] Pre-launch task '{task_label}' completed successfully"
                            ));
                            self.dap_pre_launch_done = true;
                            // Resume the debug session with the stored language.
                            if let Some(lang) = self.dap_deferred_lang.take() {
                                self.dap_start_debug(&lang);
                            }
                        } else {
                            self.dap_output_lines
                                .push(format!("[dap] Pre-launch task '{task_label}' FAILED"));
                            self.message =
                                format!("Pre-launch task '{task_label}' failed — debug aborted");
                            self.dap_session_active = false;
                            self.debug_toolbar_visible = false;
                            self.dap_deferred_lang = None;
                        }
                        redraw = true;
                    } else if let Some(adapter_name) = lang_id.strip_prefix("dap:") {
                        if success {
                            self.message = format!(
                                "DAP adapter '{adapter_name}' installed — press F5 to debug"
                            );
                        } else {
                            let short =
                                output.lines().next().unwrap_or("unknown error").to_string();
                            self.message =
                                format!("DAP install failed for '{adapter_name}': {short}");
                        }
                        redraw = true;
                    } else if success {
                        // LSP install (from :ExtInstall): look up binary from extension manifest
                        // lang_id format is "ext:{ext_name}:lsp"
                        let ext_name = lang_id
                            .strip_prefix("ext:")
                            .and_then(|s| s.strip_suffix(":lsp"))
                            .unwrap_or(&lang_id);
                        let binary = self
                            .ext_available_manifests()
                            .into_iter()
                            .find(|m| m.name == ext_name)
                            .map(|m| m.lsp.binary.clone())
                            .unwrap_or_default();
                        if !binary.is_empty() {
                            // Register the binary so future files auto-start the server.
                            // Use the manifest's args (e.g. ["--stdio"]) so the
                            // server actually communicates correctly.
                            let manifest_args = self
                                .ext_available_manifests()
                                .into_iter()
                                .find(|m| m.name == ext_name)
                                .map(|m| m.lsp.args.clone())
                                .unwrap_or_default();
                            for lsp_lang in self
                                .ext_available_manifests()
                                .into_iter()
                                .find(|m| m.name == ext_name)
                                .map(|m| m.language_ids.clone())
                                .unwrap_or_default()
                            {
                                let config = lsp::LspServerConfig {
                                    command: binary.clone(),
                                    args: manifest_args.clone(),
                                    languages: vec![lsp_lang.clone()],
                                };
                                if let Some(mgr) = &mut self.lsp_manager {
                                    mgr.add_registry_entry(config);
                                    mgr.ensure_server_for_language(&lsp_lang);
                                }
                                self.lsp_reopen_buffers_for_language(&lsp_lang);
                            }
                            self.message = format!(
                                "LSP server for '{ext_name}' installed and started ({binary})"
                            );
                            redraw = true;
                        } else {
                            self.message = format!(
                                "LSP for '{ext_name}' installed — reopen a file to activate"
                            );
                        }
                    } else {
                        let short = output.lines().next().unwrap_or("unknown error").to_string();
                        self.message = format!("LSP install failed: {short}");
                    }
                }
                LspEvent::ReferencesResponse { locations, .. } => {
                    self.lsp_pending_references = None;
                    if locations.is_empty() {
                        self.message = "No references found".to_string();
                    } else if locations.len() == 1 {
                        // Single result — jump directly like gd
                        let loc = &locations[0];
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                    } else {
                        // Multiple results — populate quickfix window
                        self.quickfix_items = locations
                            .into_iter()
                            .map(|l| ProjectMatch {
                                file: l.path,
                                line: l.range.start.line as usize,
                                col: l.range.start.character as usize,
                                line_text: String::new(),
                            })
                            .collect();
                        self.quickfix_selected = 0;
                        self.quickfix_open = true;
                        self.quickfix_has_focus = false;
                        self.message = format!("{} references found", self.quickfix_items.len());
                    }
                    redraw = true;
                }
                LspEvent::ImplementationResponse { locations, .. } => {
                    self.lsp_pending_implementation = None;
                    self.message.clear();
                    if let Some(loc) = locations.first() {
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                        redraw = true;
                    } else {
                        self.message = "No implementation found".to_string();
                    }
                }
                LspEvent::TypeDefinitionResponse { locations, .. } => {
                    self.lsp_pending_type_definition = None;
                    self.message.clear();
                    if let Some(loc) = locations.first() {
                        let path = loc.path.clone();
                        let line = loc.range.start.line as usize;
                        if path
                            != self
                                .buffer_manager
                                .get(self.active_buffer_id())
                                .and_then(|s| s.file_path.clone())
                                .unwrap_or_default()
                        {
                            let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                        }
                        self.view_mut().cursor.line = line;
                        let line_text: String = self.buffer().content.line(line).chars().collect();
                        let col = lsp::utf16_offset_to_char(&line_text, loc.range.start.character);
                        self.view_mut().cursor.col = col;
                        self.ensure_cursor_visible();
                        redraw = true;
                    } else {
                        self.message = "No type definition found".to_string();
                    }
                }
                LspEvent::SignatureHelpResponse {
                    request_id,
                    label,
                    params,
                    active_param,
                    ..
                } => {
                    if self.lsp_pending_signature == Some(request_id) {
                        self.lsp_pending_signature = None;
                        if !label.is_empty() {
                            self.lsp_signature_help = Some(SignatureHelpData {
                                label,
                                params,
                                active_param,
                            });
                        }
                        redraw = true;
                    }
                }
                LspEvent::FormattingResponse {
                    request_id, edits, ..
                } => {
                    if self.lsp_pending_formatting == Some(request_id) {
                        self.lsp_pending_formatting = None;
                        let buffer_id = self.active_buffer_id();
                        let had_edits = !edits.is_empty();
                        if had_edits {
                            self.apply_lsp_edits(buffer_id, edits);
                            // Mark buffer dirty so lsp_flush_changes sends didChange
                            // and re-requests semantic tokens on the next poll tick.
                            self.lsp_dirty_buffers.insert(buffer_id, true);
                        }
                        // If this was a format-on-save, perform the actual save now.
                        if self.format_on_save_pending.take() == Some(buffer_id) {
                            let _ = self.save();
                            if self.quit_after_format_save {
                                self.quit_after_format_save = false;
                                self.format_save_quit_ready = true;
                            }
                        } else if had_edits {
                            self.message = "Buffer formatted".to_string();
                        } else {
                            self.message = "No formatting changes".to_string();
                        }
                        redraw = true;
                    }
                }
                LspEvent::RenameResponse {
                    request_id,
                    workspace_edit,
                    error_message,
                    ..
                } => {
                    if self.lsp_pending_rename == Some(request_id) {
                        self.lsp_pending_rename = None;
                        let n = workspace_edit.changes.len();
                        if n > 0 {
                            self.apply_workspace_edit(workspace_edit);
                            self.message = format!("Renamed in {n} file(s)");
                        } else if let Some(err) = error_message {
                            self.message = format!("Rename failed: {err}");
                        } else {
                            self.message = "Rename: no changes returned by server".to_string();
                        }
                        redraw = true;
                    }
                }
                LspEvent::SemanticTokensResponse {
                    server_id,
                    request_id,
                    raw_data,
                } => {
                    if let Some(path) = self.lsp_pending_semantic_tokens.remove(&request_id) {
                        // Decode using the cached legend for this server.
                        // If the legend is missing (server restart, cache miss), keep
                        // existing tokens rather than silently replacing with empty.
                        if let Some(decoded) = self
                            .lsp_manager
                            .as_ref()
                            .and_then(|mgr| mgr.semantic_legend_for_server(server_id))
                            .map(|legend| lsp::decode_semantic_tokens(&raw_data, legend))
                        {
                            // Store on the matching buffer.
                            for &bid in self.buffer_manager.list().iter() {
                                if let Some(state) = self.buffer_manager.get_mut(bid) {
                                    if state.file_path.as_deref() == Some(path.as_path()) {
                                        state.semantic_tokens = decoded;
                                        redraw = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                LspEvent::CodeActionResponse {
                    request_id,
                    actions,
                    ..
                } => {
                    if self.lsp_pending_code_action == Some(request_id) {
                        self.lsp_pending_code_action = None;
                        let show_popup = self.lsp_show_code_action_popup_pending;
                        self.lsp_show_code_action_popup_pending = false;
                        if let Some((path, line)) = self.lsp_code_action_request_ctx.take() {
                            self.lsp_code_actions
                                .entry(path)
                                .or_default()
                                .insert(line, actions.clone());
                            if show_popup {
                                if actions.is_empty() {
                                    self.message = "No code actions available".to_string();
                                } else {
                                    self.show_code_actions_hover(line, actions);
                                }
                            }
                            redraw = true;
                        }
                    }
                }
            }
        }
        redraw
    }

    /// Request LSP completion at cursor position.
    fn lsp_request_completion(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_completion(&path, line, col_utf16) {
                self.lsp_pending_completion = Some(id);
            }
        }
    }

    /// Request LSP go-to-definition at cursor position.
    pub fn lsp_request_definition(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_definition(&path, line, col_utf16) {
                self.lsp_pending_definition = Some(id);
                self.message = "Jumping to definition...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Return which LSP navigation commands are available for the current buffer.
    /// Returns a list of (label, keybind, command_url) triples.
    fn lsp_goto_links(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let mut result = Vec::new();
        if !self.settings.lsp_enabled {
            return result;
        }
        let Some(path) = self.active_buffer_path() else {
            return result;
        };
        let Some(mgr) = &self.lsp_manager else {
            return result;
        };
        if mgr.server_supports(&path, "definitionProvider") {
            result.push(("Definition", "gd", "command:definition"));
        }
        if mgr.server_supports(&path, "typeDefinitionProvider") {
            result.push(("Type Definition", "gy", "command:type_definition"));
        }
        if mgr.server_supports(&path, "implementationProvider") {
            result.push(("Implementations", "gi", "command:implementation"));
        }
        if mgr.server_supports(&path, "referencesProvider") {
            result.push(("References", "gr", "command:references"));
        }
        result
    }

    /// Extract clickable links from rendered markdown.
    ///
    /// Pairs each `Link` span (the label text) with the following `LinkUrl` span
    /// (the URL) on the same line. The returned click region covers the label,
    /// while the URL is used for dispatch. Command URIs displayed as `:Name?args`
    /// are restored to `command:Name?args`.
    fn extract_hover_links(
        rendered: &crate::core::markdown::MdRendered,
    ) -> Vec<(usize, usize, usize, String)> {
        use crate::core::markdown::MdStyle;
        let mut links = Vec::new();
        for (line_idx, line_spans) in rendered.spans.iter().enumerate() {
            let Some(line) = rendered.lines.get(line_idx) else {
                continue;
            };
            // Find each Link span and pair it with the next LinkUrl on the same line.
            let mut span_iter = line_spans.iter().peekable();
            while let Some(span) = span_iter.next() {
                if span.style == MdStyle::Link {
                    // Look for the following LinkUrl span to get the URL.
                    let url = span_iter
                        .peek()
                        .filter(|next| next.style == MdStyle::LinkUrl)
                        .and_then(|next| {
                            if next.end_byte <= line.len() {
                                Some(&line[next.start_byte..next.end_byte])
                            } else {
                                None
                            }
                        });
                    if let Some(url_text) = url {
                        // Command URIs display as ":Name?args" — restore prefix.
                        let url = if url_text.starts_with(':') {
                            format!("command{}", url_text)
                        } else {
                            url_text.to_string()
                        };
                        if is_safe_url(&url) {
                            // Click region = the Link label span.
                            links.push((line_idx, span.start_byte, span.end_byte, url));
                        }
                    }
                }
            }
        }
        links
    }

    /// Execute an LSP navigation command from a hover popup link.
    /// Moves the cursor to the given position before invoking the LSP request.
    pub fn execute_hover_goto(&mut self, command: &str) {
        // Get the anchor position from the hover popup before dismissing it.
        let (line, col) = if let Some(hover) = &self.editor_hover {
            (hover.anchor_line, hover.anchor_col)
        } else {
            return;
        };
        self.dismiss_editor_hover();
        // Move cursor to the hover anchor position.
        let view = self.view_mut();
        view.cursor.line = line;
        view.cursor.col = col;
        self.push_jump_location();
        match command {
            "command:definition" => self.lsp_request_definition(),
            "command:type_definition" => self.lsp_request_type_definition(),
            "command:implementation" => self.lsp_request_implementation(),
            "command:references" => self.lsp_request_references(),
            _ => {
                // Try dispatching as a command URI to plugin commands.
                self.execute_command_uri(command);
            }
        }
    }

    /// Decode percent-encoded characters in a string (e.g. `%20` → space).
    pub fn percent_decode(input: &str) -> String {
        let mut result = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    result.push((hi << 4 | lo) as char);
                    i += 3;
                    continue;
                }
            }
            result.push(bytes[i] as char);
            i += 1;
        }
        result
    }

    /// Execute a `command:Name` or `command:Name?args` URI.
    /// Returns `true` if a matching command was found and executed.
    pub fn execute_command_uri(&mut self, url: &str) -> bool {
        let rest = match url.strip_prefix("command:") {
            Some(r) => r,
            None => return false,
        };
        if rest.is_empty() {
            return false;
        }
        let (cmd_name, cmd_args) = match rest.split_once('?') {
            Some((name, args)) => (name, Self::percent_decode(args)),
            None => (rest, String::new()),
        };
        if cmd_name.is_empty() {
            return false;
        }
        self.plugin_run_command(cmd_name, &cmd_args)
    }

    /// Request LSP hover at cursor position.
    /// Request LSP hover at a specific buffer position (not necessarily the cursor).
    fn lsp_request_hover_at(&mut self, line: usize, col: usize) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let Some(state) = self.buffer_manager.get(self.active_buffer_id()) else {
            return;
        };
        let Some(path) = state.file_path.as_ref().cloned() else {
            return;
        };
        let line_text: String = state.buffer.content.line(line).chars().collect();
        let col_utf16 = lsp::char_to_utf16_offset(&line_text, col);
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_hover(&path, line as u32, col_utf16) {
                self.lsp_pending_hover = Some(id);
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request code actions at the exact cursor position.
    /// Called proactively after cursor settles (150ms debounce) and on-demand via `<leader>ca`.
    pub fn lsp_request_code_actions_for_line(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        // Don't send if a request is already in flight.
        if self.lsp_pending_code_action.is_some() {
            return;
        }
        self.ensure_lsp_manager();
        let Some((path, lsp_line, col_utf16)) = self.lsp_cursor_position() else {
            return;
        };
        let line = lsp_line as usize;
        // Clear stale cache for this line — actions depend on exact column.
        if let Some(line_map) = self.lsp_code_actions.get_mut(&path) {
            line_map.remove(&line);
        }
        // Build diagnostics JSON for lines touching the cursor line.
        let diags_json = self.diagnostics_json_for_line(&path, line);
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_code_action(&path, lsp_line, col_utf16, diags_json) {
                self.lsp_pending_code_action = Some(id);
                self.lsp_code_action_last_line = Some((path.clone(), line));
                self.lsp_code_action_request_ctx = Some((path, line));
            }
        }
    }

    /// Build a JSON array of diagnostics touching a specific line (for code action context).
    fn diagnostics_json_for_line(&self, path: &Path, line: usize) -> serde_json::Value {
        let diags = match self.lsp_diagnostics.get(path) {
            Some(d) => d,
            None => return serde_json::json!([]),
        };
        let arr: Vec<serde_json::Value> = diags
            .iter()
            .filter(|d| {
                let start = d.range.start.line as usize;
                let end = d.range.end.line as usize;
                line >= start && line <= end
            })
            .map(|d| {
                serde_json::json!({
                    "range": {
                        "start": { "line": d.range.start.line, "character": d.range.start.character },
                        "end": { "line": d.range.end.line, "character": d.range.end.character }
                    },
                    "severity": d.severity as i32,
                    "message": d.message
                })
            })
            .collect();
        serde_json::Value::Array(arr)
    }

    /// Whether any code actions are available on the given line.
    pub fn has_code_actions_on_line(&self, line: usize) -> bool {
        let Some(path) = self.active_buffer_path() else {
            return false;
        };
        self.lsp_code_actions
            .get(&path)
            .and_then(|m| m.get(&line))
            .is_some_and(|v| !v.is_empty())
    }

    /// Show code actions for the current line in an editor hover popup.
    /// If cached actions exist, shows immediately. Otherwise fires an LSP request
    /// and `lsp_show_code_action_popup_pending` causes the response handler to
    /// display the popup when results arrive.
    pub fn show_code_actions_popup(&mut self) {
        if self.active_buffer_path().is_none() {
            return;
        }
        // Always make a fresh request — code actions depend on exact cursor column.
        self.lsp_show_code_action_popup_pending = true;
        self.lsp_request_code_actions_for_line();
        if self.lsp_pending_code_action.is_none() {
            self.lsp_show_code_action_popup_pending = false;
            self.message = "No code actions available".to_string();
        }
    }

    fn show_code_actions_hover(&mut self, _line: usize, actions: Vec<lsp::CodeAction>) {
        let buttons: Vec<DialogButton> = actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let kind_str = a
                    .kind
                    .as_deref()
                    .map(|k| format!(" ({})", k))
                    .unwrap_or_default();
                DialogButton {
                    label: format!("{}{}", a.title, kind_str),
                    hotkey: '\0', // no single-key hotkey
                    action: format!("apply_{}", i),
                }
            })
            .collect();
        self.pending_code_action_choices = actions;
        self.show_dialog("code_actions", "Code Actions", vec![], buttons);
    }

    /// Request LSP find-references at cursor position.
    pub fn lsp_request_references(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_references(&path, line, col_utf16) {
                self.lsp_pending_references = Some(id);
                self.message = "Finding references...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP go-to-implementation at cursor position.
    fn lsp_request_implementation(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_implementation(&path, line, col_utf16) {
                self.lsp_pending_implementation = Some(id);
                self.message = "Finding implementation...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP go-to-type-definition at cursor position.
    fn lsp_request_type_definition(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_type_definition(&path, line, col_utf16) {
                self.lsp_pending_type_definition = Some(id);
                self.message = "Finding type definition...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP signature help at cursor position (triggered in insert mode).
    fn lsp_request_signature_help(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_signature_help(&path, line, col_utf16) {
                self.lsp_pending_signature = Some(id);
            }
        }
    }

    /// Request LSP formatting for the current buffer.
    pub fn lsp_format_current(&mut self) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, _line, _col) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        let tab_size = self.settings.tabstop as u32;
        let insert_spaces = self.settings.expand_tab;
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_formatting(&path, tab_size, insert_spaces) {
                self.lsp_pending_formatting = Some(id);
                self.message = "Formatting...".to_string();
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Request LSP rename of the symbol at cursor.
    fn lsp_request_rename(&mut self, new_name: &str) {
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let (path, line, col_utf16) = match self.lsp_cursor_position() {
            Some(v) => v,
            None => return,
        };
        let new_name = new_name.to_string();
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_rename(&path, line, col_utf16, &new_name) {
                self.lsp_pending_rename = Some(id);
                self.message = format!("Renaming to '{new_name}'...");
            } else if mgr.is_server_initializing(&path) {
                self.message = "LSP server initializing...".to_string();
            } else {
                self.message = "No LSP server for this file".to_string();
            }
        }
    }

    /// Apply a list of LSP text edits to a buffer as a single undo group.
    /// Edits must be applied in reverse order (last first) to preserve offsets.
    fn apply_lsp_edits(&mut self, buffer_id: BufferId, mut edits: Vec<FormattingEdit>) {
        if edits.is_empty() {
            return;
        }
        // Sort in reverse start order so applying one edit doesn't shift others
        edits.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        // Start undo group on the target buffer (not necessarily the active buffer).
        let cursor = self
            .windows
            .values()
            .find(|w| w.buffer_id == buffer_id)
            .map(|w| w.view.cursor)
            .unwrap_or_default();
        if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
            state.start_undo_group(cursor);
        }
        for edit in &edits {
            let state = match self.buffer_manager.get(buffer_id) {
                Some(s) => s,
                None => break,
            };
            let content = state.buffer.content.clone();
            let total_lines = content.len_lines();
            let start_line = (edit.range.start.line as usize).min(total_lines.saturating_sub(1));
            let end_line = (edit.range.end.line as usize).min(total_lines.saturating_sub(1));

            let start_line_text: String = content.line(start_line).chars().collect();
            let end_line_text: String = content.line(end_line).chars().collect();

            let start_char =
                lsp::utf16_offset_to_char(&start_line_text, edit.range.start.character);
            let end_char = lsp::utf16_offset_to_char(&end_line_text, edit.range.end.character);

            let start_offset = content.line_to_char(start_line) + start_char;
            let end_offset = content.line_to_char(end_line) + end_char;

            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                if end_offset > start_offset {
                    let deleted: String = state
                        .buffer
                        .content
                        .slice(start_offset..end_offset)
                        .chars()
                        .collect();
                    state.buffer.content.remove(start_offset..end_offset);
                    state.record_delete(start_offset, &deleted);
                }
                if !edit.new_text.is_empty() {
                    state.buffer.content.insert(start_offset, &edit.new_text);
                    state.record_insert(start_offset, &edit.new_text);
                }
                state.dirty = true;
            }
        }
        if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
            state.finish_undo_group();
            // Clear stale semantic tokens immediately — positions are now wrong.
            state.semantic_tokens.clear();
        }
        // Mark buffer dirty so the next LSP flush sends didChange + re-requests tokens.
        self.lsp_dirty_buffers.insert(buffer_id, true);
    }

    /// Apply a workspace-wide rename edit.
    fn apply_workspace_edit(&mut self, we: WorkspaceEdit) {
        for file_edit in we.changes {
            // Try to find an already-open buffer for this path
            let buffer_id = self.buffer_manager.list().into_iter().find(|&bid| {
                self.buffer_manager
                    .get(bid)
                    .and_then(|s| s.file_path.as_deref())
                    .map(|p| p == file_edit.path)
                    .unwrap_or(false)
            });

            if let Some(bid) = buffer_id {
                self.apply_lsp_edits(bid, file_edit.edits);
            } else {
                // File not open — read, edit, and write back to disk
                if let Ok(text) = std::fs::read_to_string(&file_edit.path) {
                    let mut edits = file_edit.edits;
                    // Sort in reverse order
                    edits.sort_by(|a, b| {
                        b.range
                            .start
                            .line
                            .cmp(&a.range.start.line)
                            .then(b.range.start.character.cmp(&a.range.start.character))
                    });
                    let mut rope = ropey::Rope::from_str(&text);
                    for edit in &edits {
                        let total_lines = rope.len_lines();
                        let start_line =
                            (edit.range.start.line as usize).min(total_lines.saturating_sub(1));
                        let end_line =
                            (edit.range.end.line as usize).min(total_lines.saturating_sub(1));
                        let start_line_text: String = rope.line(start_line).chars().collect();
                        let end_line_text: String = rope.line(end_line).chars().collect();
                        let start_char =
                            lsp::utf16_offset_to_char(&start_line_text, edit.range.start.character);
                        let end_char =
                            lsp::utf16_offset_to_char(&end_line_text, edit.range.end.character);
                        let start_offset = rope.line_to_char(start_line) + start_char;
                        let end_offset = rope.line_to_char(end_line) + end_char;
                        if end_offset > start_offset {
                            rope.remove(start_offset..end_offset);
                        }
                        rope.insert(start_offset, &edit.new_text);
                    }
                    let _ = std::fs::write(&file_edit.path, rope.to_string());
                }
            }
        }
    }

    /// Get the cursor's file path, line, and UTF-16 column for LSP requests.
    fn lsp_cursor_position(&self) -> Option<(PathBuf, u32, u32)> {
        let state = self.buffer_manager.get(self.active_buffer_id())?;
        let path = state.file_path.as_ref()?.clone();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_text: String = state.buffer.content.line(line).chars().collect();
        let col_utf16 = lsp::char_to_utf16_offset(&line_text, col);
        Some((path, line as u32, col_utf16))
    }

    /// Jump to the next diagnostic in the current buffer.
    pub fn jump_next_diagnostic(&mut self) {
        let path = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let path = match path {
            Some(p) => p,
            None => return,
        };
        let diags = match self.lsp_diagnostics.get(&path) {
            Some(d) if !d.is_empty() => d,
            _ => {
                self.message = "No diagnostics".to_string();
                return;
            }
        };
        let cur_line = self.view().cursor.line as u32;
        let cur_char = self.view().cursor.col as u32;

        // Find the first diagnostic after the current cursor position
        let next = diags.iter().find(|d| {
            d.range.start.line > cur_line
                || (d.range.start.line == cur_line && d.range.start.character > cur_char)
        });
        let diag = next.unwrap_or(&diags[0]).clone();

        let line = diag.range.start.line as usize;
        self.view_mut().cursor.line = line;
        let line_text: String = self.buffer().content.line(line).chars().collect();
        self.view_mut().cursor.col =
            lsp::utf16_offset_to_char(&line_text, diag.range.start.character);
        self.message = format!("{}: {}", diag.severity.symbol(), diag.message);
    }

    /// Jump to the previous diagnostic in the current buffer.
    pub fn jump_prev_diagnostic(&mut self) {
        let path = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let path = match path {
            Some(p) => p,
            None => return,
        };
        let diags = match self.lsp_diagnostics.get(&path) {
            Some(d) if !d.is_empty() => d,
            _ => {
                self.message = "No diagnostics".to_string();
                return;
            }
        };
        let cur_line = self.view().cursor.line as u32;
        let cur_char = self.view().cursor.col as u32;

        // Find the last diagnostic before the current cursor position
        let prev = diags.iter().rev().find(|d| {
            d.range.start.line < cur_line
                || (d.range.start.line == cur_line && d.range.start.character < cur_char)
        });
        let diag = prev.unwrap_or(diags.last().unwrap()).clone();

        let line = diag.range.start.line as usize;
        self.view_mut().cursor.line = line;
        let line_text: String = self.buffer().content.line(line).chars().collect();
        self.view_mut().cursor.col =
            lsp::utf16_offset_to_char(&line_text, diag.range.start.character);
        self.message = format!("{}: {}", diag.severity.symbol(), diag.message);
    }

    /// Shut down all LSP servers (called on quit).
    pub fn lsp_shutdown(&mut self) {
        if let Some(mgr) = &mut self.lsp_manager {
            mgr.shutdown_all();
        }
        self.lsp_manager = None;
    }

    /// Get diagnostic counts for the current buffer (for status bar).
    pub fn diagnostic_counts(&self) -> (usize, usize) {
        let path = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
        let path = match path {
            Some(p) => p,
            None => return (0, 0),
        };
        let diags = match self.lsp_diagnostics.get(&path) {
            Some(d) => d,
            None => return (0, 0),
        };
        let errors = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
        let warnings = diags
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count();
        (errors, warnings)
    }

    // =======================================================================
    // Toggle case (~)
    // =======================================================================

    /// Toggle the case of `count` characters starting at the cursor, advance cursor.
    fn toggle_case_at_cursor(&mut self, count: usize, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(line) + col;

        // How many chars are available on this line (excluding trailing newline)?
        let line_len = self.buffer().line_len_chars(line);
        let line_content = self.buffer().content.line(line);
        let available = if line_content.chars().last() == Some('\n') {
            line_len.saturating_sub(1)
        } else {
            line_len
        };
        let remaining = available.saturating_sub(col);
        let to_toggle = count.min(remaining);

        if to_toggle == 0 {
            return;
        }

        // Read chars to toggle
        let chars: Vec<char> = self
            .buffer()
            .content
            .slice(char_idx..char_idx + to_toggle)
            .chars()
            .collect();

        // Build replacement: toggle case of each char
        let toggled: String = chars
            .iter()
            .map(|&c| {
                if c.is_uppercase() {
                    c.to_lowercase().next().unwrap_or(c)
                } else if c.is_lowercase() {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();

        self.start_undo_group();
        self.delete_with_undo(char_idx, char_idx + to_toggle);
        self.insert_with_undo(char_idx, &toggled);
        self.finish_undo_group();

        // Advance cursor by number of chars toggled (clamped to line end)
        let new_col = (col + to_toggle).min(available.saturating_sub(1));
        self.view_mut().cursor.col = new_col;
        self.clamp_cursor_col();
        *changed = true;
    }

    // =======================================================================
    // Join lines (J)
    // =======================================================================

    /// Join `count` lines starting at cursor. Collapses the newline + leading
    /// whitespace of the next line into a single space (no space before `)`).
    fn join_lines(&mut self, count: usize, changed: &mut bool) {
        let total_lines = self.buffer().len_lines();
        let start_line = self.view().cursor.line;

        // We join (count) times; each join merges current line with next
        let joins = count.min(total_lines.saturating_sub(start_line + 1));
        if joins == 0 {
            return;
        }

        self.start_undo_group();
        for _ in 0..joins {
            let cur_line = self.view().cursor.line;
            let next_line = cur_line + 1;
            if next_line >= self.buffer().len_lines() {
                break;
            }

            // Find position of newline at end of current line
            let cur_line_len = self.buffer().line_len_chars(cur_line);
            let cur_line_start = self.buffer().line_to_char(cur_line);
            // The newline is the last char of the current line
            let newline_pos = cur_line_start + cur_line_len - 1;

            // Count leading whitespace on next line
            let next_line_start = self.buffer().line_to_char(next_line);
            let next_line_content: String = self.buffer().content.line(next_line).chars().collect();
            let leading_ws = next_line_content
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();

            // Determine what char comes after the whitespace on the next line
            let next_non_ws = next_line_content.chars().nth(leading_ws);

            // Delete: newline + leading whitespace of next line
            let del_end = next_line_start + leading_ws;
            self.delete_with_undo(newline_pos, del_end);

            // Insert a space unless the next non-ws char is ')' or next line was empty/only ws
            // Also don't add space if the current line ends with a space
            let should_add_space = !matches!(next_non_ws, None | Some(')') | Some(']') | Some('}'));
            // Check if current line ends with space (after the newline was removed)
            let cur_end_char =
                self.buffer().line_to_char(cur_line) + self.buffer().line_len_chars(cur_line);
            let ends_with_space = cur_end_char > self.buffer().line_to_char(cur_line)
                && self.buffer().content.char(cur_end_char - 1) == ' ';

            if should_add_space && !ends_with_space {
                self.insert_with_undo(newline_pos, " ");
            }
        }
        self.finish_undo_group();

        // Cursor stays at start of original line
        self.clamp_cursor_col();
        *changed = true;
    }

    // =======================================================================
    // Scroll cursor to position (zz / zt / zb)
    // =======================================================================

    /// Scroll so that cursor line is centered in viewport.
    fn scroll_cursor_center(&mut self) {
        let cursor_line = self.view().cursor.line;
        let half = self.viewport_lines() / 2;
        let new_top = cursor_line.saturating_sub(half);
        self.view_mut().scroll_top = new_top;
    }

    /// Scroll so that cursor line is at the top of viewport.
    fn scroll_cursor_top(&mut self) {
        let cursor_line = self.view().cursor.line;
        self.view_mut().scroll_top = cursor_line;
    }

    /// Scroll so that cursor line is at the bottom of viewport.
    fn scroll_cursor_bottom(&mut self) {
        let cursor_line = self.view().cursor.line;
        let viewport = self.viewport_lines();
        let new_top = cursor_line.saturating_sub(viewport.saturating_sub(1));
        self.view_mut().scroll_top = new_top;
    }

    // =======================================================================
    // Search word under cursor (* / #)
    // =======================================================================

    /// Extract the word under the cursor. Returns None if cursor is not on a word char.
    fn word_under_cursor(&self) -> Option<String> {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_content: String = self.buffer().content.line(line).chars().collect();
        let chars: Vec<char> = line_content.chars().collect();

        if col >= chars.len() {
            return None;
        }
        if !Self::is_word_char(chars[col]) {
            return None;
        }

        // Find start of word
        let start = (0..=col)
            .rev()
            .take_while(|&i| Self::is_word_char(chars[i]))
            .last()
            .unwrap_or(col);
        // Find end of word (exclusive)
        let end = (col..chars.len())
            .take_while(|&i| Self::is_word_char(chars[i]))
            .last()
            .map(|i| i + 1)
            .unwrap_or(col + 1);

        Some(chars[start..end].iter().collect())
    }

    /// Search forward (*) or backward (#) for the word under cursor with word boundaries.
    fn search_word_under_cursor(&mut self, forward: bool) {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return;
            }
        };

        self.search_query = word.clone();
        self.search_direction = if forward {
            SearchDirection::Forward
        } else {
            SearchDirection::Backward
        };
        self.search_word_bounded = true;

        // Build word-boundary matches manually
        self.build_word_bounded_matches();

        if self.search_matches.is_empty() {
            self.message = format!("Pattern not found: {}", word);
            return;
        }

        // Jump to first match in the appropriate direction
        if forward {
            self.search_next();
        } else {
            self.search_prev();
        }
    }

    /// Like run_search but only keeps matches that are whole words.
    fn build_word_bounded_matches(&mut self) {
        self.search_matches.clear();
        self.search_index = None;

        if self.search_query.is_empty() {
            return;
        }

        let text = self.buffer().to_string();
        let query = self.search_query.clone();
        let mut byte_pos = 0;

        while let Some(found) = text[byte_pos..].find(&query) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query.len();

            // Check word boundaries
            let before_ok = start_byte == 0 || {
                let c = text[..start_byte].chars().last().unwrap_or(' ');
                !Self::is_word_char(c)
            };
            let after_ok = end_byte >= text.len() || {
                let c = text[end_byte..].chars().next().unwrap_or(' ');
                !Self::is_word_char(c)
            };

            if before_ok && after_ok {
                let start_char = self.buffer().content.byte_to_char(start_byte);
                let end_char = self.buffer().content.byte_to_char(end_byte);
                self.search_matches.push((start_char, end_char));
            }

            byte_pos = start_byte + 1;
        }
    }

    // =======================================================================
    // Multiple cursors (Alt-D)
    // =======================================================================

    /// Convert a char index in the current buffer into a `Cursor` (line, col).
    fn char_idx_to_cursor(&self, char_idx: usize) -> Cursor {
        let len = self.buffer().content.len_chars();
        let char_idx = char_idx.min(len);
        if len == 0 {
            return Cursor { line: 0, col: 0 };
        }
        let line = self.buffer().content.char_to_line(char_idx);
        let line_start = self.buffer().line_to_char(line);
        Cursor {
            line,
            col: char_idx - line_start,
        }
    }

    /// Convert a byte offset in the buffer text into a `Cursor`.
    fn byte_offset_to_cursor(&self, byte_offset: usize) -> Cursor {
        let char_idx = self.buffer().content.byte_to_char(byte_offset);
        self.char_idx_to_cursor(char_idx)
    }

    /// Search for the next occurrence of `pattern` in the buffer, starting
    /// one pattern-length past `after`.  Wraps around the document end.
    /// Returns `None` if `pattern` is not found anywhere in the buffer.
    fn find_next_occurrence(
        &self,
        pattern: &str,
        after: Cursor,
        word_bounded: bool,
    ) -> Option<Cursor> {
        if pattern.is_empty() {
            return None;
        }

        let text = self.buffer().to_string();
        // Start searching one pattern-length past the given cursor position.
        let after_char_idx =
            self.buffer().line_to_char(after.line) + after.col + pattern.chars().count();
        let after_byte = self
            .buffer()
            .content
            .char_to_byte(after_char_idx.min(self.buffer().content.len_chars()));

        let check_boundary = |sb: usize, eb: usize| -> bool {
            if !word_bounded {
                return true;
            }
            let before_ok =
                sb == 0 || !Self::is_word_char(text[..sb].chars().last().unwrap_or(' '));
            let after_ok =
                eb >= text.len() || !Self::is_word_char(text[eb..].chars().next().unwrap_or(' '));
            before_ok && after_ok
        };

        // Pass 1: from after_byte to end of document.
        let mut byte_pos = after_byte;
        while byte_pos < text.len() {
            match text[byte_pos..].find(pattern) {
                None => break,
                Some(found) => {
                    let sb = byte_pos + found;
                    let eb = sb + pattern.len();
                    if check_boundary(sb, eb) {
                        return Some(self.byte_offset_to_cursor(sb));
                    }
                    byte_pos = sb + 1;
                }
            }
        }

        // Pass 2: wrap around from document start to after_byte.
        byte_pos = 0;
        while byte_pos < after_byte {
            match text[byte_pos..].find(pattern) {
                None => break,
                Some(found) => {
                    let sb = byte_pos + found;
                    if sb >= after_byte {
                        break;
                    }
                    let eb = sb + pattern.len();
                    if check_boundary(sb, eb) {
                        return Some(self.byte_offset_to_cursor(sb));
                    }
                    byte_pos = sb + 1;
                }
            }
        }

        None
    }

    /// Collect all byte-offset positions of `pattern` in the current buffer,
    /// returning them as `Cursor` values.  When `word_bounded` is true only
    /// whole-word matches are returned.
    fn collect_all_occurrences(&self, pattern: &str, word_bounded: bool) -> Vec<Cursor> {
        if pattern.is_empty() {
            return vec![];
        }
        let text = self.buffer().to_string();
        let mut results = Vec::new();
        let mut byte_pos = 0;
        while byte_pos < text.len() {
            match text[byte_pos..].find(pattern) {
                None => break,
                Some(found) => {
                    let sb = byte_pos + found;
                    let eb = sb + pattern.len();
                    let ok = if word_bounded {
                        let before_ok = sb == 0
                            || !Self::is_word_char(text[..sb].chars().last().unwrap_or(' '));
                        let after_ok = eb >= text.len()
                            || !Self::is_word_char(text[eb..].chars().next().unwrap_or(' '));
                        before_ok && after_ok
                    } else {
                        true
                    };
                    if ok {
                        results.push(self.byte_offset_to_cursor(sb));
                    }
                    byte_pos = sb + 1;
                }
            }
        }
        results
    }

    /// Add secondary cursors at *every* occurrence of the word under the
    /// primary cursor.  Called by backends when `select_all_matches` is pressed.
    pub fn select_all_word_occurrences(&mut self) -> EngineAction {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return EngineAction::None;
            }
        };
        let all = self.collect_all_occurrences(&word, true);
        if all.is_empty() {
            self.message = format!("No occurrences of '{}'", word);
            return EngineAction::None;
        }
        let primary = *self.cursor();
        let extras: Vec<Cursor> = all.into_iter().filter(|&c| c != primary).collect();
        let n = extras.len();
        self.view_mut().extra_cursors = extras;
        self.message = format!("{} cursors (all occurrences of '{}')", n + 1, word);
        EngineAction::None
    }

    /// Add a secondary cursor at the given `(line, col)` position.
    /// Does nothing if the position equals the primary cursor or is already
    /// present in `extra_cursors`.
    pub fn add_cursor_at_pos(&mut self, line: usize, col: usize) {
        let new_cursor = Cursor { line, col };
        if new_cursor == *self.cursor() {
            return;
        }
        if self.view().extra_cursors.contains(&new_cursor) {
            return;
        }
        self.view_mut().extra_cursors.push(new_cursor);
    }

    /// Add a secondary cursor at the next occurrence of the word under the
    /// primary cursor (or after the last extra cursor if any exist).
    /// Called by backends when the configured `add_cursor` key is pressed.
    pub fn add_cursor_at_next_match(&mut self) -> EngineAction {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return EngineAction::None;
            }
        };
        let search_after = self
            .view()
            .extra_cursors
            .last()
            .copied()
            .unwrap_or_else(|| *self.cursor());
        if let Some(new_cursor) = self.find_next_occurrence(&word, search_after, true) {
            let is_primary = new_cursor == *self.cursor();
            let already_extra = self.view().extra_cursors.contains(&new_cursor);
            if !is_primary && !already_extra {
                self.view_mut().extra_cursors.push(new_cursor);
                let total = self.view().extra_cursors.len() + 1; // +1 for primary
                self.message = format!("{} cursors ('{}')", total, word);
            } else {
                self.message = format!("No more occurrences of '{}'", word);
            }
        } else {
            self.message = format!("No more occurrences of '{}'", word);
        }
        EngineAction::None
    }

    // ── Multi-cursor editing helpers ─────────────────────────────────────────

    /// Insert `text` at every cursor position (primary + extra) simultaneously.
    /// Processes in ascending char-index order with a running offset so that
    /// each subsequent insert uses the correct adjusted position.
    /// Updates primary cursor and all extra cursors to point just after their
    /// respective inserted text.
    fn mc_insert(&mut self, text: &str) {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;
        let extra_origs: Vec<usize> = extra
            .iter()
            .map(|c| self.buffer().line_to_char(c.line) + c.col)
            .collect();

        // All original char indices sorted ascending (safe to sort since positions are distinct).
        let mut all_origs: Vec<usize> = extra_origs.clone();
        all_origs.push(primary_orig);
        all_origs.sort_unstable();

        let insert_chars = text.chars().count();

        // Pre-compute new char indices before modifying the buffer.
        // Cursor at ascending rank i → new_cidx = orig + (rank+1)*insert_chars.
        let rank_of = |orig: usize| all_origs.iter().position(|&x| x == orig).unwrap_or(0);
        let primary_new_cidx = primary_orig + (rank_of(primary_orig) + 1) * insert_chars;
        let extra_new_cidxs: Vec<usize> = extra_origs
            .iter()
            .map(|&orig| orig + (rank_of(orig) + 1) * insert_chars)
            .collect();

        // Insert in ascending order with cumulative offset.
        let mut offset = 0usize;
        for &orig in &all_origs {
            self.insert_with_undo(orig + offset, text);
            offset += insert_chars;
        }

        // Apply updated positions (buffer is now modified; char_idx_to_cursor uses new state).
        self.view_mut().cursor = self.char_idx_to_cursor(primary_new_cidx);
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .map(|&cidx| self.char_idx_to_cursor(cidx))
            .collect();
    }

    /// Delete one char before every cursor position with col > 0.
    /// Extra cursors at col == 0 are left in place (line-merge not done in multi-cursor mode).
    /// Returns `true` if at least one deletion was performed.
    fn mc_backspace(&mut self) -> bool {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;

        // Pre-compute original char indices for extra cursors (before any modification).
        let extra_data: Vec<(usize, bool)> = extra
            .iter()
            .map(|c| {
                let orig = self.buffer().line_to_char(c.line) + c.col;
                (orig, c.col > 0)
            })
            .collect();

        // Collect eligible (col > 0) original char indices.
        let mut all_eligible: Vec<usize> = Vec::new();
        let primary_eligible = primary.col > 0;
        if primary_eligible {
            all_eligible.push(primary_orig);
        }
        for &(orig, eligible) in &extra_data {
            if eligible {
                all_eligible.push(orig);
            }
        }

        if all_eligible.is_empty() {
            return false;
        }

        all_eligible.sort_unstable();

        // Pre-compute new char indices before modifying the buffer.
        // Cursor at ascending rank i → new_cidx = orig - (rank+1).
        let rank_of = |orig: usize| all_eligible.iter().position(|&x| x == orig).unwrap_or(0);
        let primary_new_cidx = if primary_eligible {
            Some(primary_orig - (rank_of(primary_orig) + 1))
        } else {
            None
        };
        let extra_new_cidxs: Vec<Option<usize>> = extra_data
            .iter()
            .map(|&(orig, eligible)| {
                if eligible {
                    Some(orig - (rank_of(orig) + 1))
                } else {
                    None
                }
            })
            .collect();

        // Delete in DESCENDING order (no offset adjustment needed).
        for &orig in all_eligible.iter().rev() {
            self.delete_with_undo(orig - 1, orig);
        }

        // Apply updated positions.
        if let Some(new_cidx) = primary_new_cidx {
            self.view_mut().cursor = self.char_idx_to_cursor(new_cidx);
        }
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .zip(extra.iter())
            .map(|(&opt, ec)| {
                if let Some(new_cidx) = opt {
                    self.char_idx_to_cursor(new_cidx)
                } else {
                    *ec // unchanged (was at col == 0)
                }
            })
            .collect();

        true
    }

    /// Delete one char after every cursor position that is not at end-of-buffer.
    /// Returns `true` if at least one deletion was performed.
    fn mc_delete_forward(&mut self) -> bool {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        let buf_len = self.buffer().content.len_chars();
        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;

        let extra_data: Vec<(usize, bool)> = extra
            .iter()
            .map(|c| {
                let orig = self.buffer().line_to_char(c.line) + c.col;
                (orig, orig < buf_len)
            })
            .collect();

        let mut all_eligible: Vec<usize> = Vec::new();
        let primary_eligible = primary_orig < buf_len;
        if primary_eligible {
            all_eligible.push(primary_orig);
        }
        for &(orig, eligible) in &extra_data {
            if eligible {
                all_eligible.push(orig);
            }
        }

        if all_eligible.is_empty() {
            return false;
        }

        all_eligible.sort_unstable();

        // Pre-compute new char indices.
        // Delete-forward: cursor stays in place; earlier deletions shift it left.
        // Cursor at ascending rank i → new_cidx = orig - rank (not rank+1).
        let rank_of = |orig: usize| all_eligible.iter().position(|&x| x == orig).unwrap_or(0);
        let primary_new_cidx = if primary_eligible {
            Some(primary_orig - rank_of(primary_orig))
        } else {
            None
        };
        let extra_new_cidxs: Vec<Option<usize>> = extra_data
            .iter()
            .map(|&(orig, eligible)| {
                if eligible {
                    Some(orig - rank_of(orig))
                } else {
                    None
                }
            })
            .collect();

        // Delete in DESCENDING order.
        for &orig in all_eligible.iter().rev() {
            self.delete_with_undo(orig, orig + 1);
        }

        if let Some(new_cidx) = primary_new_cidx {
            self.view_mut().cursor = self.char_idx_to_cursor(new_cidx);
        }
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .zip(extra.iter())
            .map(|(&opt, ec)| {
                if let Some(new_cidx) = opt {
                    self.char_idx_to_cursor(new_cidx)
                } else {
                    *ec
                }
            })
            .collect();

        true
    }

    /// Insert a newline (+ auto-indent) at every cursor position.
    /// Each cursor gets the indent of its own line computed before any modification.
    fn mc_return(&mut self) {
        let extra = self.view().extra_cursors.clone();
        let primary = *self.cursor();

        // Pre-compute (orig_cidx, insert_text) for every cursor, ascending.
        struct ReturnOp {
            orig_cidx: usize,
            text: String,
            is_primary: bool,
            extra_idx: usize,
        }

        let primary_indent = if self.settings.auto_indent {
            self.get_line_indent_str(primary.line)
        } else {
            String::new()
        };

        let extra_ops: Vec<(usize, String)> = extra
            .iter()
            .map(|c| {
                let orig = self.buffer().line_to_char(c.line) + c.col;
                let indent = if self.settings.auto_indent {
                    self.get_line_indent_str(c.line)
                } else {
                    String::new()
                };
                (orig, format!("\n{}", indent))
            })
            .collect();

        let primary_orig = self.buffer().line_to_char(primary.line) + primary.col;
        let primary_text = format!("\n{}", primary_indent);

        let mut all_ops: Vec<ReturnOp> = extra_ops
            .iter()
            .enumerate()
            .map(|(i, (orig, text))| ReturnOp {
                orig_cidx: *orig,
                text: text.clone(),
                is_primary: false,
                extra_idx: i,
            })
            .collect();
        all_ops.push(ReturnOp {
            orig_cidx: primary_orig,
            text: primary_text,
            is_primary: true,
            extra_idx: 0,
        });
        all_ops.sort_by_key(|op| op.orig_cidx);

        // Apply inserts ascending with cumulative offset; cursor goes to end of each insert.
        let mut running_offset = 0usize;
        let mut primary_new_cidx = 0usize;
        let mut extra_new_cidxs = vec![0usize; extra.len()];

        for op in &all_ops {
            let text_chars = op.text.chars().count();
            let insert_at = op.orig_cidx + running_offset;
            self.insert_with_undo(insert_at, &op.text);
            let new_cidx = insert_at + text_chars;
            running_offset += text_chars;
            if op.is_primary {
                primary_new_cidx = new_cidx;
            } else {
                extra_new_cidxs[op.extra_idx] = new_cidx;
            }
        }

        self.view_mut().cursor = self.char_idx_to_cursor(primary_new_cidx);
        self.view_mut().extra_cursors = extra_new_cidxs
            .iter()
            .map(|&cidx| self.char_idx_to_cursor(cidx))
            .collect();
    }

    // =======================================================================
    // Jump list (Ctrl-O / Ctrl-I)
    // =======================================================================

    /// Push (line, col) to the change list, capped at 100 entries.
    fn push_change_location(&mut self, line: usize, col: usize) {
        // Truncate any forward entries (if we navigated back with g;)
        self.change_list.truncate(self.change_list_pos);
        // Avoid duplicate consecutive entries
        if self.change_list.last() == Some(&(line, col)) {
            return;
        }
        self.change_list.push((line, col));
        if self.change_list.len() > 100 {
            self.change_list.remove(0);
        }
        self.change_list_pos = self.change_list.len();
    }

    /// Push the current cursor position onto the jump list.
    pub fn push_jump_location(&mut self) {
        // Save pre-jump position for '' / `` marks
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        self.last_jump_pos = Some((line, col));

        let file = self.active_buffer_state().file_path.clone();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;

        // Truncate forward history when a new jump is made
        if self.jump_list_pos < self.jump_list.len() {
            self.jump_list.truncate(self.jump_list_pos);
        }

        // Don't push a duplicate of the current top entry
        if let Some(last) = self.jump_list.last() {
            if last.0 == file && last.1 == line && last.2 == col {
                return;
            }
        }

        self.jump_list.push((file, line, col));

        // Cap at 100 entries
        if self.jump_list.len() > 100 {
            self.jump_list.remove(0);
        }

        self.jump_list_pos = self.jump_list.len();
    }

    /// Navigate backward in the jump list (Ctrl-O).
    pub fn jump_list_back(&mut self) {
        // When at the "live" end (not stored in list), save current position
        // so Ctrl-I can return to it, then jump to the previous entry.
        if self.jump_list_pos == self.jump_list.len() {
            if self.jump_list.is_empty() {
                self.message = "Already at oldest position in jump list".to_string();
                return;
            }
            let file = self.active_buffer_state().file_path.clone();
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            #[allow(clippy::unnecessary_map_or)] // is_none_or requires Rust 1.82+
            let should_push = self.jump_list.last().map_or(true, |last| {
                last.0 != file || last.1 != line || last.2 != col
            });
            if should_push {
                self.jump_list.push((file, line, col));
                if self.jump_list.len() > 100 {
                    self.jump_list.remove(0);
                }
            }
            // Jump to the entry BEFORE the one we just saved
            // (list.len()-1 is current, list.len()-2 is the previous)
            if self.jump_list.len() < 2 {
                self.message = "Already at oldest position in jump list".to_string();
                return;
            }
            self.jump_list_pos = self.jump_list.len() - 2;
            self.apply_jump_list_entry(self.jump_list_pos);
            return;
        }

        // We're inside the list — go to the previous entry
        if self.jump_list_pos == 0 {
            self.message = "Already at oldest position in jump list".to_string();
            return;
        }

        self.jump_list_pos -= 1;
        self.apply_jump_list_entry(self.jump_list_pos);
    }

    /// Navigate forward in the jump list (Ctrl-I / Tab).
    pub fn jump_list_forward(&mut self) {
        if self.jump_list_pos + 1 >= self.jump_list.len() {
            self.message = "Already at newest position in jump list".to_string();
            return;
        }

        self.jump_list_pos += 1;
        self.apply_jump_list_entry(self.jump_list_pos);
    }

    /// Move to the position stored at the given jump list index.
    fn apply_jump_list_entry(&mut self, idx: usize) {
        let entry = match self.jump_list.get(idx) {
            Some(e) => e.clone(),
            None => return,
        };

        let (file, line, col) = entry;

        // If cross-file, open the file
        let current_file = self.active_buffer_state().file_path.clone();
        if file != current_file {
            if let Some(path) = &file {
                let path = path.clone();
                let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
            }
        }

        let max_line = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = line.min(max_line);
        self.view_mut().cursor.col = col;
        self.clamp_cursor_col();
    }

    // =======================================================================
    // Indent / Dedent (>> / <<)
    // =======================================================================

    /// Indent `count` lines starting at `start_line` by shift_width.
    fn indent_lines(&mut self, start_line: usize, count: usize, changed: &mut bool) {
        let indent_str = if self.settings.expand_tab {
            " ".repeat(self.settings.shift_width as usize)
        } else {
            "\t".to_string()
        };

        self.start_undo_group();
        let total = self.buffer().len_lines();
        for i in 0..count {
            let line_idx = start_line + i;
            if line_idx >= total {
                break;
            }
            let line_start = self.buffer().line_to_char(line_idx);
            self.insert_with_undo(line_start, &indent_str);
        }
        self.finish_undo_group();
        *changed = true;
    }

    /// Dedent `count` lines starting at `start_line` by up to shift_width.
    fn dedent_lines(&mut self, start_line: usize, count: usize, changed: &mut bool) {
        let sw = self.settings.shift_width as usize;
        self.start_undo_group();
        // Work backwards to avoid invalidating positions
        let total = self.buffer().len_lines();
        for i in (0..count).rev() {
            let line_idx = start_line + i;
            if line_idx >= total {
                continue;
            }
            let line_start = self.buffer().line_to_char(line_idx);
            let line_content: String = self.buffer().content.line(line_idx).chars().collect();
            let mut removed = 0;
            for ch in line_content.chars() {
                if removed >= sw {
                    break;
                }
                match ch {
                    ' ' => removed += 1,
                    '\t' => removed += sw.min(sw - (removed % sw).max(1) + 1).min(sw - removed),
                    _ => break,
                }
            }
            if removed > 0 {
                self.delete_with_undo(line_start, line_start + removed);
            }
        }
        self.finish_undo_group();
        if count > 0 {
            *changed = true;
        }
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
mod keys;
mod motions;
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
