use super::*;

// ─── Theme ────────────────────────────────────────────────────────────────────

/// All colours used by the editor UI.
/// Derive new themes by constructing a `Theme` with different field values.
pub struct Theme {
    // Editor background
    pub background: Color,
    /// Slightly lighter background for the active window when splits exist.
    pub active_background: Color,
    /// Default text foreground.
    pub foreground: Color,

    // Syntax highlighting
    pub keyword: Color,
    pub string_lit: Color,
    pub comment: Color,
    pub function: Color,
    pub type_name: Color,
    pub variable: Color,
    pub number: Color,
    pub control_flow: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub macro_call: Color,
    pub attribute: Color,
    pub lifetime: Color,
    pub constant: Color,
    pub escape: Color,
    pub boolean: Color,
    pub property: Color,
    pub parameter: Color,
    pub module: Color,
    /// Fallback foreground for unrecognised scopes.
    pub default_fg: Color,

    // Visual selection (alpha handled separately in Cairo)
    pub selection: Color,
    pub selection_alpha: f64,

    // Cursor
    pub cursor: Color,
    pub cursor_normal_alpha: f64,

    // Search match highlights
    pub search_match_bg: Color,
    pub search_current_match_bg: Color,
    pub search_match_fg: Color,

    // Yank highlight flash
    pub yank_highlight_bg: Color,
    pub yank_highlight_alpha: f64,

    // Virtual text / line annotations (e.g. git blame inline)
    pub annotation_fg: Color,

    // AI ghost text (inline completions)
    pub ghost_text_fg: Color,

    // Tab bar
    pub tab_bar_bg: Color,
    pub tab_active_bg: Color,
    pub tab_active_fg: Color,
    pub tab_inactive_fg: Color,
    pub tab_preview_active_fg: Color,
    pub tab_preview_inactive_fg: Color,
    /// Accent line color for the active tab in the focused editor group.
    pub tab_active_accent: Color,

    // Status line
    pub status_bg: Color,
    pub status_fg: Color,

    // Per-window status line mode text tints
    pub status_mode_normal_bg: Color,
    pub status_mode_insert_bg: Color,
    pub status_mode_visual_bg: Color,
    pub status_mode_replace_bg: Color,
    pub status_inactive_bg: Color,
    pub status_inactive_fg: Color,

    // Wildmenu (command Tab completion bar)
    pub wildmenu_bg: Color,
    pub wildmenu_fg: Color,
    pub wildmenu_sel_bg: Color,
    pub wildmenu_sel_fg: Color,

    // Command / message line
    pub command_bg: Color,
    pub command_fg: Color,

    // Line numbers
    pub line_number_fg: Color,
    pub line_number_active_fg: Color,

    // Window separator
    pub separator: Color,

    // Git diff gutter markers
    pub git_added: Color,
    pub git_modified: Color,
    pub git_deleted: Color,

    // Completion popup
    pub completion_bg: Color,
    pub completion_selected_bg: Color,
    pub completion_fg: Color,
    pub completion_border: Color,

    // Diagnostic colours
    pub diagnostic_error: Color,
    pub diagnostic_warning: Color,
    pub diagnostic_info: Color,
    pub diagnostic_hint: Color,

    // Spell checking
    pub spell_error: Color,

    // Code action lightbulb
    pub lightbulb: Color,

    // Hover popup
    pub hover_bg: Color,
    pub hover_fg: Color,
    pub hover_border: Color,

    // Fuzzy file-picker modal
    pub fuzzy_bg: Color,
    pub fuzzy_selected_bg: Color,
    pub fuzzy_fg: Color,
    pub fuzzy_query_fg: Color,
    pub fuzzy_border: Color,
    pub fuzzy_title_fg: Color,
    /// Highlight color for fuzzy-match character positions.
    pub fuzzy_match_fg: Color,

    // Two-way diff background colours
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_padding_bg: Color,

    // DAP stopped-line highlight
    pub dap_stopped_bg: Color,

    // Cursor line highlight (subtle background for the current line).
    // Derived from `background` by default; overridden by VSCode theme
    // `editor.lineHighlightBackground`.
    pub cursorline_bg: Color,

    // Markdown preview colours
    pub md_heading1: Color,
    pub md_heading2: Color,
    pub md_heading3: Color,
    pub md_code: Color,
    pub md_link: Color,

    // Sidebar selection
    /// Background for the selected row when the sidebar has keyboard focus.
    pub sidebar_sel_bg: Color,
    /// Background for the selected row when the sidebar does NOT have focus.
    pub sidebar_sel_bg_inactive: Color,

    // LSP semantic token colours (overlay on tree-sitter)
    pub semantic_parameter: Color,
    pub semantic_property: Color,
    pub semantic_namespace: Color,
    pub semantic_enum_member: Color,
    pub semantic_interface: Color,
    pub semantic_type_parameter: Color,
    pub semantic_decorator: Color,
    pub semantic_macro: Color,

    // Breadcrumb bar
    pub breadcrumb_bg: Color,
    pub breadcrumb_fg: Color,
    pub breadcrumb_active_fg: Color,

    // Indent guides
    pub indent_guide_fg: Color,
    pub indent_guide_active_fg: Color,

    // Color column (`:set colorcolumn=80`)
    pub colorcolumn_bg: Color,

    // Bracket match highlight
    pub bracket_match_bg: Color,

    // Explorer sidebar (TUI)
    /// Foreground for directory names in the file explorer.
    pub explorer_dir_fg: Color,
    /// Foreground for file names in the file explorer (muted grey).
    pub explorer_file_fg: Color,
    /// Background tint for rows whose file is open in a buffer.
    pub explorer_active_bg: Color,

    // Scrollbar
    /// Scrollbar thumb (draggable part).
    pub scrollbar_thumb: Color,
    /// Scrollbar track (gutter behind thumb).
    pub scrollbar_track: Color,

    // Integrated terminal
    /// Default background for the integrated terminal pane.
    pub terminal_bg: Color,

    // Activity bar
    /// Foreground for activity bar icons.
    pub activity_bar_fg: Color,
    /// Colour of the 2px left-edge accent line on the active activity-bar
    /// item (`quadraui::ActivityBar::active_accent`, #658). Mirrors VS
    /// Code's `activityBar.activeBorder`.
    pub activity_active_accent: Color,
}

impl Theme {
    /// The OneDark-inspired colour scheme currently used by VimCode.
    /// All values are derived directly from the Cairo RGB tuples in the
    /// original `draw_*` functions.
    pub fn onedark() -> Self {
        let bg = hex("#1a1a1a");
        Self {
            // (0.1, 0.1, 0.1)
            background: bg,
            // (0.12, 0.12, 0.12)
            active_background: hex("#1e1e1e"),
            // (0.9, 0.9, 0.9)
            foreground: hex("#e5e5e5"),

            keyword: hex("#c678dd"),
            control_flow: hex("#c678dd"),
            string_lit: hex("#98c379"),
            comment: hex("#5c6370"),
            function: hex("#61afef"),
            type_name: hex("#e5c07b"),
            variable: hex("#e06c75"),
            number: hex("#d19a66"),
            operator: hex("#56b6c2"),
            punctuation: hex("#abb2bf"),
            macro_call: hex("#61afef"),
            attribute: hex("#e5c07b"),
            lifetime: hex("#e06c75"),
            constant: hex("#d19a66"),
            escape: hex("#56b6c2"),
            boolean: hex("#d19a66"),
            property: hex("#e06c75"),
            parameter: hex("#e06c75"),
            module: hex("#e5c07b"),
            default_fg: hex("#abb2bf"),

            // (0.3, 0.5, 0.7) with alpha 0.3
            selection: hex("#4c7fb2"),
            selection_alpha: 0.3,

            // (1.0, 1.0, 1.0) with alpha 0.5 in Normal/Visual
            cursor: hex("#ffffff"),
            cursor_normal_alpha: 0.5,

            // Pango 16-bit: (180*256, 150*256, 0) → RGB(180, 150, 0)
            search_match_bg: hex("#b49600"),
            // Pango 16-bit: (255*256, 200*256, 0) → RGB(255, 200, 0)
            search_current_match_bg: hex("#ffc800"),
            search_match_fg: hex("#000000"),

            // (0.15, 0.15, 0.2)
            tab_bar_bg: hex("#262633"),
            // (0.25, 0.25, 0.35)
            tab_active_bg: hex("#3f3f59"),
            // (1.0, 1.0, 1.0)
            tab_active_fg: hex("#ffffff"),
            // (0.7, 0.7, 0.7)
            tab_inactive_fg: hex("#b2b2b2"),
            // (0.8, 0.8, 0.8)
            tab_preview_active_fg: hex("#cccccc"),
            // (0.5, 0.5, 0.5)
            tab_preview_inactive_fg: hex("#7f7f7f"),
            tab_active_accent: hex("#61afef"),

            status_bg: hex("#33334c"),
            status_fg: hex("#e5e5e5"),

            status_mode_normal_bg: hex("#61afef"),
            status_mode_insert_bg: hex("#98c379"),
            status_mode_visual_bg: hex("#c678dd"),
            status_mode_replace_bg: hex("#e06c75"),
            status_inactive_bg: hex("#262626"),
            status_inactive_fg: hex("#808080"),

            wildmenu_bg: hex("#33334c"),
            wildmenu_fg: hex("#abb2bf"),
            wildmenu_sel_bg: hex("#e5c07b"),
            wildmenu_sel_fg: hex("#282c34"),

            // (0.1, 0.1, 0.1)
            command_bg: hex("#1a1a1a"),
            // (0.9, 0.9, 0.9)
            command_fg: hex("#e5e5e5"),

            // VS Code's `editorLineNumber.foreground` (#699 Tier 2a / #701).
            // Was #b2b2b2 (0.7, 0.7, 0.7), which read brighter than the
            // editor's own body text (`default_fg` #abb2bf) and pulled the
            // eye into the gutter. The cursor's line is brightened
            // separately via `line_number_active_fg` below, so dimming the
            // inactive token *increases* the active/inactive contrast.
            line_number_fg: hex("#858585"),
            // (0.9, 0.9, 0.5)
            line_number_active_fg: hex("#e5e57f"),

            // (0.3, 0.3, 0.4)
            separator: hex("#4c4c66"),

            // Git diff gutter markers
            git_added: hex("#98c379"),    // green
            git_modified: hex("#e5c07b"), // yellow
            git_deleted: hex("#e06c75"),  // red

            // Completion popup (OneDark palette)
            completion_bg: hex("#282c34"),
            completion_selected_bg: hex("#3e4451"),
            completion_fg: hex("#abb2bf"),
            completion_border: hex("#528bff"),

            // Diagnostic colours
            diagnostic_error: hex("#e06c75"),   // red
            diagnostic_warning: hex("#e5c07b"), // yellow
            diagnostic_info: hex("#61afef"),    // blue
            diagnostic_hint: hex("#5c6370"),    // grey
            spell_error: hex("#56b6c2"),        // cyan
            lightbulb: hex("#e5c07b"),          // yellow

            // Hover popup
            hover_bg: hex("#21252b"),
            hover_fg: hex("#abb2bf"),
            hover_border: hex("#528bff"),

            // Fuzzy file-picker modal (OneDark palette)
            fuzzy_bg: hex("#21252b"),
            fuzzy_selected_bg: hex("#2c313c"),
            fuzzy_fg: hex("#abb2bf"),
            fuzzy_query_fg: hex("#61afef"),
            fuzzy_border: hex("#528bff"),
            fuzzy_title_fg: hex("#e5c07b"),
            fuzzy_match_fg: hex("#61afef"),

            // Two-way diff backgrounds — must be clearly green/red in terminals
            diff_added_bg: hex("#14541a"),
            diff_removed_bg: hex("#541a1a"),
            diff_padding_bg: hex("#2d2d2d"),

            // DAP stopped-line (dark amber)
            dap_stopped_bg: hex("#3a3000"),

            // Cursor line highlight (subtle lightening of background)
            cursorline_bg: hex("#1a1a1a").cursorline_tint(), // derived from background

            // Yank highlight flash (green, matching Neovim default)
            yank_highlight_bg: hex("#57d45e"),
            yank_highlight_alpha: 0.35,

            // Virtual text annotations (muted grey — matches comment colour)
            annotation_fg: hex("#5c6370"),

            // AI ghost text (inline completions) — slightly lighter than annotation
            ghost_text_fg: hex("#4b5263"),

            // Markdown preview
            md_heading1: hex("#e5c07b"), // gold
            md_heading2: hex("#61afef"), // blue
            md_heading3: hex("#c678dd"), // purple
            md_code: hex("#98c379"),     // green (string-like)
            md_link: hex("#61afef"),     // blue

            sidebar_sel_bg: hex("#373d4a"), // focused: visible highlight
            sidebar_sel_bg_inactive: hex("#21252b"), // unfocused: very faint
            semantic_parameter: hex("#c8ae9d"), // warm sandy (distinct from variable red)
            semantic_property: hex("#d19a66"), // orange
            semantic_namespace: hex("#e5c07b"), // gold
            semantic_enum_member: hex("#56b6c2"), // cyan
            semantic_interface: hex("#e5c07b"), // gold (like type)
            semantic_type_parameter: hex("#e5c07b"), // gold
            semantic_decorator: hex("#c678dd"), // purple (like keyword)
            semantic_macro: hex("#56b6c2"), // cyan

            breadcrumb_bg: hex("#21252b"),
            // #699 Tier 2a / #701: the non-trailing crumbs recede. Was
            // #7f848e, a straight 15% dim of which is #6c7079 — same hue,
            // now clearly below the editor's body text (`default_fg`
            // #abb2bf) so the path reads as chrome rather than competing
            // with the code. The trailing/current crumb keeps
            // `breadcrumb_active_fg` at full body-text brightness.
            breadcrumb_fg: hex("#6c7079"),
            breadcrumb_active_fg: hex("#abb2bf"),

            indent_guide_fg: hex("#404040"),
            indent_guide_active_fg: hex("#606060"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: hex("#3a3d41"),

            explorer_dir_fg: hex("#61afef"),    // function blue
            explorer_file_fg: hex("#aab1be"),   // muted grey (matches OneDark sidebar)
            explorer_active_bg: hex("#333842"), // current-file tint

            scrollbar_thumb: hex("#5a5a5a"),
            scrollbar_track: hex("#1a1a1a"),
            terminal_bg: hex("#1e1e1e"),
            activity_bar_fg: hex("#c8c8d2"),
            activity_active_accent: hex("#c8c8d2"),
        }
    }

    /// Gruvbox Dark colour scheme.
    pub fn gruvbox_dark() -> Self {
        let bg = hex("#282828");
        Self {
            background: bg,
            active_background: hex("#32302f"),
            foreground: hex("#ebdbb2"),

            keyword: hex("#fb4934"),
            control_flow: hex("#fb4934"),
            string_lit: hex("#b8bb26"),
            comment: hex("#928374"),
            function: hex("#8ec07c"),
            type_name: hex("#fabd2f"),
            variable: hex("#83a598"),
            number: hex("#d3869b"),
            operator: hex("#8ec07c"),
            punctuation: hex("#ebdbb2"),
            macro_call: hex("#8ec07c"),
            attribute: hex("#fabd2f"),
            lifetime: hex("#fb4934"),
            constant: hex("#d3869b"),
            escape: hex("#8ec07c"),
            boolean: hex("#d3869b"),
            property: hex("#83a598"),
            parameter: hex("#83a598"),
            module: hex("#fabd2f"),
            default_fg: hex("#ebdbb2"),

            selection: hex("#458588"),
            selection_alpha: 0.4,

            cursor: hex("#ebdbb2"),
            cursor_normal_alpha: 0.6,

            search_match_bg: hex("#d65d0e"),
            search_current_match_bg: hex("#fe8019"),
            search_match_fg: hex("#1d2021"),

            tab_bar_bg: hex("#3c3836"),
            tab_active_bg: hex("#504945"),
            tab_active_fg: hex("#ebdbb2"),
            tab_inactive_fg: hex("#a89984"),
            tab_preview_active_fg: hex("#d5c4a1"),
            tab_preview_inactive_fg: hex("#7c6f64"),
            tab_active_accent: hex("#d65d0e"),

            status_bg: hex("#504945"),
            status_fg: hex("#ebdbb2"),

            status_mode_normal_bg: hex("#83a598"),
            status_mode_insert_bg: hex("#b8bb26"),
            status_mode_visual_bg: hex("#d3869b"),
            status_mode_replace_bg: hex("#fb4934"),
            status_inactive_bg: hex("#303030"),
            status_inactive_fg: hex("#808080"),

            wildmenu_bg: hex("#504945"),
            wildmenu_fg: hex("#ebdbb2"),
            wildmenu_sel_bg: hex("#fabd2f"),
            wildmenu_sel_fg: hex("#282828"),

            command_bg: hex("#282828"),
            command_fg: hex("#ebdbb2"),

            line_number_fg: hex("#7c6f64"),
            line_number_active_fg: hex("#fabd2f"),

            separator: hex("#665c54"),

            git_added: hex("#b8bb26"),
            git_modified: hex("#fabd2f"),
            git_deleted: hex("#fb4934"),

            completion_bg: hex("#32302f"),
            completion_selected_bg: hex("#504945"),
            completion_fg: hex("#ebdbb2"),
            completion_border: hex("#458588"),

            diagnostic_error: hex("#fb4934"),
            diagnostic_warning: hex("#fabd2f"),
            diagnostic_info: hex("#83a598"),
            diagnostic_hint: hex("#928374"),
            spell_error: hex("#8ec07c"),
            lightbulb: hex("#fabd2f"),

            hover_bg: hex("#32302f"),
            hover_fg: hex("#ebdbb2"),
            hover_border: hex("#458588"),

            fuzzy_bg: hex("#32302f"),
            fuzzy_selected_bg: hex("#504945"),
            fuzzy_fg: hex("#ebdbb2"),
            fuzzy_query_fg: hex("#8ec07c"),
            fuzzy_border: hex("#458588"),
            fuzzy_title_fg: hex("#fabd2f"),
            fuzzy_match_fg: hex("#83a598"),

            // (bg #282828)
            diff_added_bg: hex("#1e5e24"),
            diff_removed_bg: hex("#5e2424"),
            diff_padding_bg: hex("#333333"),

            dap_stopped_bg: hex("#3a3000"),

            cursorline_bg: hex("#282828").cursorline_tint(), // derived from background

            yank_highlight_bg: hex("#b8bb26"),
            yank_highlight_alpha: 0.35,

            annotation_fg: hex("#928374"),
            ghost_text_fg: hex("#7c6f64"),

            md_heading1: hex("#fabd2f"),
            md_heading2: hex("#83a598"),
            md_heading3: hex("#d3869b"),
            md_code: hex("#b8bb26"),
            md_link: hex("#83a598"),

            sidebar_sel_bg: hex("#504945"), // focused: visible highlight
            sidebar_sel_bg_inactive: hex("#32302f"), // unfocused
            semantic_parameter: hex("#83a598"), // blue
            semantic_property: hex("#d3869b"), // purple-pink
            semantic_namespace: hex("#fabd2f"), // yellow
            semantic_enum_member: hex("#8ec07c"), // aqua
            semantic_interface: hex("#fabd2f"), // yellow
            semantic_type_parameter: hex("#fabd2f"),
            semantic_decorator: hex("#fb4934"), // red
            semantic_macro: hex("#8ec07c"),     // aqua

            breadcrumb_bg: hex("#32302f"),
            breadcrumb_fg: hex("#a89984"),
            breadcrumb_active_fg: hex("#ebdbb2"),

            indent_guide_fg: hex("#3c3836"),
            indent_guide_active_fg: hex("#504945"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: hex("#504945"),

            explorer_dir_fg: hex("#83a598"),    // gruvbox blue
            explorer_file_fg: hex("#bdae93"),   // gruvbox muted
            explorer_active_bg: hex("#45403d"), // current-file tint

            scrollbar_thumb: hex("#665c54"),
            scrollbar_track: hex("#282828"),
            terminal_bg: hex("#282828"),
            activity_bar_fg: hex("#bdae93"),
            activity_active_accent: hex("#bdae93"),
        }
    }

    /// Tokyo Night colour scheme.
    pub fn tokyo_night() -> Self {
        let bg = hex("#1a1b26");
        Self {
            background: bg,
            active_background: hex("#1f2335"),
            foreground: hex("#c0caf5"),

            keyword: hex("#bb9af7"),
            control_flow: hex("#bb9af7"),
            string_lit: hex("#9ece6a"),
            comment: hex("#565f89"),
            function: hex("#7aa2f7"),
            type_name: hex("#e0af68"),
            variable: hex("#f7768e"),
            number: hex("#ff9e64"),
            operator: hex("#89ddff"),
            punctuation: hex("#a9b1d6"),
            macro_call: hex("#7aa2f7"),
            attribute: hex("#e0af68"),
            lifetime: hex("#f7768e"),
            constant: hex("#ff9e64"),
            escape: hex("#89ddff"),
            boolean: hex("#ff9e64"),
            property: hex("#73daca"),
            parameter: hex("#e0af68"),
            module: hex("#e0af68"),
            default_fg: hex("#a9b1d6"),

            selection: hex("#364a82"),
            selection_alpha: 0.5,

            cursor: hex("#c0caf5"),
            cursor_normal_alpha: 0.5,

            search_match_bg: hex("#3d59a1"),
            search_current_match_bg: hex("#ff9e64"),
            search_match_fg: hex("#c0caf5"),

            tab_bar_bg: hex("#16161e"),
            tab_active_bg: hex("#292e42"),
            tab_active_fg: hex("#c0caf5"),
            tab_inactive_fg: hex("#545c7e"),
            tab_preview_active_fg: hex("#a9b1d6"),
            tab_preview_inactive_fg: hex("#3b4261"),
            tab_active_accent: hex("#7aa2f7"),

            status_bg: hex("#292e42"),
            status_fg: hex("#c0caf5"),

            status_mode_normal_bg: hex("#7aa2f7"),
            status_mode_insert_bg: hex("#9ece6a"),
            status_mode_visual_bg: hex("#bb9af7"),
            status_mode_replace_bg: hex("#f7768e"),
            status_inactive_bg: hex("#262626"),
            status_inactive_fg: hex("#808080"),

            wildmenu_bg: hex("#292e42"),
            wildmenu_fg: hex("#c0caf5"),
            wildmenu_sel_bg: hex("#e0af68"),
            wildmenu_sel_fg: hex("#1a1b26"),

            command_bg: hex("#1a1b26"),
            command_fg: hex("#c0caf5"),

            line_number_fg: hex("#3b4261"),
            line_number_active_fg: hex("#e0af68"),

            separator: hex("#292e42"),

            git_added: hex("#9ece6a"),
            git_modified: hex("#e0af68"),
            git_deleted: hex("#f7768e"),

            completion_bg: hex("#1f2335"),
            completion_selected_bg: hex("#364a82"),
            completion_fg: hex("#c0caf5"),
            completion_border: hex("#7aa2f7"),

            diagnostic_error: hex("#f7768e"),
            diagnostic_warning: hex("#e0af68"),
            diagnostic_info: hex("#7aa2f7"),
            diagnostic_hint: hex("#565f89"),
            spell_error: hex("#7dcfff"),
            lightbulb: hex("#e0af68"),

            hover_bg: hex("#1f2335"),
            hover_fg: hex("#c0caf5"),
            hover_border: hex("#7aa2f7"),

            fuzzy_bg: hex("#1f2335"),
            fuzzy_selected_bg: hex("#364a82"),
            fuzzy_fg: hex("#c0caf5"),
            fuzzy_query_fg: hex("#7aa2f7"),
            fuzzy_border: hex("#7aa2f7"),
            fuzzy_title_fg: hex("#e0af68"),
            fuzzy_match_fg: hex("#7aa2f7"),

            // (bg #1a1b26)
            diff_added_bg: hex("#14541a"),
            diff_removed_bg: hex("#541a28"),
            diff_padding_bg: hex("#252530"),

            dap_stopped_bg: hex("#2a2500"),

            cursorline_bg: hex("#1a1b26").cursorline_tint(), // derived from background

            yank_highlight_bg: hex("#9ece6a"),
            yank_highlight_alpha: 0.35,

            annotation_fg: hex("#565f89"),
            ghost_text_fg: hex("#414868"),

            md_heading1: hex("#e0af68"),
            md_heading2: hex("#7aa2f7"),
            md_heading3: hex("#bb9af7"),
            md_code: hex("#9ece6a"),
            md_link: hex("#7aa2f7"),

            sidebar_sel_bg: hex("#33395a"), // focused: visible highlight
            sidebar_sel_bg_inactive: hex("#1f2335"), // unfocused
            semantic_parameter: hex("#e0af68"), // orange-gold
            semantic_property: hex("#73daca"), // teal
            semantic_namespace: hex("#2ac3de"), // cyan
            semantic_enum_member: hex("#ff9e64"), // orange
            semantic_interface: hex("#2ac3de"), // cyan
            semantic_type_parameter: hex("#e0af68"),
            semantic_decorator: hex("#bb9af7"), // purple
            semantic_macro: hex("#2ac3de"),     // cyan

            breadcrumb_bg: hex("#1f2335"),
            breadcrumb_fg: hex("#565f89"),
            breadcrumb_active_fg: hex("#c0caf5"),

            indent_guide_fg: hex("#292e42"),
            indent_guide_active_fg: hex("#3b4261"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: hex("#364a82"),

            explorer_dir_fg: hex("#7aa2f7"),    // tokyo blue
            explorer_file_fg: hex("#a9b1d6"),   // tokyo muted
            explorer_active_bg: hex("#2f3550"), // current-file tint

            scrollbar_thumb: hex("#565f89"),
            scrollbar_track: hex("#1a1b26"),
            terminal_bg: hex("#1a1b26"),
            activity_bar_fg: hex("#a9b1d6"),
            activity_active_accent: hex("#a9b1d6"),
        }
    }

    /// Solarized Dark colour scheme.
    pub fn solarized_dark() -> Self {
        let bg = hex("#002b36");
        Self {
            background: bg,
            active_background: hex("#073642"),
            foreground: hex("#839496"),

            keyword: hex("#859900"),
            control_flow: hex("#859900"),
            string_lit: hex("#2aa198"),
            comment: hex("#586e75"),
            function: hex("#268bd2"),
            type_name: hex("#b58900"),
            variable: hex("#dc322f"),
            number: hex("#2aa198"),
            operator: hex("#859900"),
            punctuation: hex("#93a1a1"),
            macro_call: hex("#268bd2"),
            attribute: hex("#b58900"),
            lifetime: hex("#dc322f"),
            constant: hex("#2aa198"),
            escape: hex("#cb4b16"),
            boolean: hex("#2aa198"),
            property: hex("#268bd2"),
            parameter: hex("#93a1a1"),
            module: hex("#b58900"),
            default_fg: hex("#93a1a1"),

            selection: hex("#073642"),
            selection_alpha: 0.6,

            cursor: hex("#93a1a1"),
            cursor_normal_alpha: 0.6,

            search_match_bg: hex("#cb4b16"),
            search_current_match_bg: hex("#d33682"),
            search_match_fg: hex("#fdf6e3"),

            tab_bar_bg: hex("#073642"),
            tab_active_bg: hex("#0d4a5a"),
            tab_active_fg: hex("#93a1a1"),
            tab_inactive_fg: hex("#586e75"),
            tab_preview_active_fg: hex("#839496"),
            tab_preview_inactive_fg: hex("#4a6570"),
            tab_active_accent: hex("#268bd2"),

            status_bg: hex("#073642"),
            status_fg: hex("#93a1a1"),

            status_mode_normal_bg: hex("#268bd2"),
            status_mode_insert_bg: hex("#859900"),
            status_mode_visual_bg: hex("#6c71c4"),
            status_mode_replace_bg: hex("#dc322f"),
            status_inactive_bg: hex("#121212"),
            status_inactive_fg: hex("#6c6c6c"),

            wildmenu_bg: hex("#073642"),
            wildmenu_fg: hex("#93a1a1"),
            wildmenu_sel_bg: hex("#b58900"),
            wildmenu_sel_fg: hex("#002b36"),

            command_bg: hex("#002b36"),
            command_fg: hex("#839496"),

            line_number_fg: hex("#586e75"),
            line_number_active_fg: hex("#b58900"),

            separator: hex("#073642"),

            git_added: hex("#859900"),
            git_modified: hex("#b58900"),
            git_deleted: hex("#dc322f"),

            completion_bg: hex("#073642"),
            completion_selected_bg: hex("#0d4a5a"),
            completion_fg: hex("#839496"),
            completion_border: hex("#268bd2"),

            diagnostic_error: hex("#dc322f"),
            diagnostic_warning: hex("#b58900"),
            diagnostic_info: hex("#268bd2"),
            diagnostic_hint: hex("#586e75"),
            spell_error: hex("#2aa198"),
            lightbulb: hex("#b58900"),

            hover_bg: hex("#073642"),
            hover_fg: hex("#93a1a1"),
            hover_border: hex("#268bd2"),

            fuzzy_bg: hex("#073642"),
            fuzzy_selected_bg: hex("#0d4a5a"),
            fuzzy_fg: hex("#839496"),
            fuzzy_query_fg: hex("#268bd2"),
            fuzzy_border: hex("#268bd2"),
            fuzzy_title_fg: hex("#b58900"),
            fuzzy_match_fg: hex("#268bd2"),

            // (bg #002b36)
            diff_added_bg: hex("#005e30"),
            diff_removed_bg: hex("#5e1a28"),
            diff_padding_bg: hex("#0a3545"),

            dap_stopped_bg: hex("#2b2000"),

            cursorline_bg: hex("#002b36").cursorline_tint(), // derived from background

            yank_highlight_bg: hex("#859900"),
            yank_highlight_alpha: 0.35,

            annotation_fg: hex("#586e75"),
            ghost_text_fg: hex("#4a5e68"),

            md_heading1: hex("#b58900"),
            md_heading2: hex("#268bd2"),
            md_heading3: hex("#6c71c4"),
            md_code: hex("#859900"),
            md_link: hex("#268bd2"),

            sidebar_sel_bg: hex("#0a4a5a"), // focused: visible highlight
            sidebar_sel_bg_inactive: hex("#002b36"), // unfocused (base03)
            semantic_parameter: hex("#268bd2"), // blue
            semantic_property: hex("#2aa198"), // cyan
            semantic_namespace: hex("#b58900"), // yellow
            semantic_enum_member: hex("#cb4b16"), // orange
            semantic_interface: hex("#b58900"), // yellow
            semantic_type_parameter: hex("#b58900"),
            semantic_decorator: hex("#6c71c4"), // violet
            semantic_macro: hex("#d33682"),     // magenta

            breadcrumb_bg: hex("#073642"),
            breadcrumb_fg: hex("#586e75"),
            breadcrumb_active_fg: hex("#93a1a1"),

            indent_guide_fg: hex("#073642"),
            indent_guide_active_fg: hex("#0d4a5a"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: hex("#0d4a5a"),

            explorer_dir_fg: hex("#268bd2"),    // solarized blue
            explorer_file_fg: hex("#93a1a1"),   // solarized base1
            explorer_active_bg: hex("#0a4050"), // current-file tint

            scrollbar_thumb: hex("#586e75"),
            scrollbar_track: hex("#002b36"),
            terminal_bg: hex("#002b36"),
            activity_bar_fg: hex("#93a1a1"),
            activity_active_accent: hex("#93a1a1"),
        }
    }

    /// VSCode Dark+ colour scheme.
    pub fn vscode_dark() -> Self {
        let bg = hex("#1e1e1e");
        Self {
            background: bg,
            active_background: hex("#252526"),
            foreground: hex("#d4d4d4"),

            keyword: hex("#569cd6"),      // blue (storage: let, fn, struct)
            control_flow: hex("#c586c0"), // purple (if, else, for, return)
            string_lit: hex("#ce9178"),   // salmon
            comment: hex("#6a9955"),      // green
            function: hex("#dcdcaa"),     // yellow
            type_name: hex("#4ec9b0"),    // teal
            variable: hex("#9cdcfe"),     // light blue
            number: hex("#b5cea8"),       // light green
            operator: hex("#d4d4d4"),
            punctuation: hex("#d4d4d4"),
            macro_call: hex("#dcdcaa"),
            attribute: hex("#4ec9b0"),
            lifetime: hex("#569cd6"),
            constant: hex("#4fc1ff"),
            escape: hex("#d7ba7d"),
            boolean: hex("#569cd6"),
            property: hex("#9cdcfe"),
            parameter: hex("#9cdcfe"),
            module: hex("#4ec9b0"),
            default_fg: hex("#d4d4d4"),

            selection: hex("#264f78"),
            selection_alpha: 0.6,

            cursor: hex("#aeafad"),
            cursor_normal_alpha: 0.6,

            search_match_bg: hex("#515c6a"),
            search_current_match_bg: hex("#613214"),
            search_match_fg: hex("#d4d4d4"),

            tab_bar_bg: hex("#252526"),
            tab_active_bg: hex("#1e1e1e"),
            tab_active_fg: hex("#ffffff"),
            tab_inactive_fg: hex("#969696"),
            tab_preview_active_fg: hex("#cccccc"),
            tab_preview_inactive_fg: hex("#7f7f7f"),
            tab_active_accent: hex("#007acc"),

            status_bg: hex("#007acc"),
            status_fg: hex("#ffffff"),

            status_mode_normal_bg: hex("#007acc"),
            status_mode_insert_bg: hex("#16825d"),
            status_mode_visual_bg: hex("#68217a"),
            status_mode_replace_bg: hex("#c72e0f"),
            status_inactive_bg: hex("#262626"),
            status_inactive_fg: hex("#808080"),

            wildmenu_bg: hex("#252526"),
            wildmenu_fg: hex("#d4d4d4"),
            wildmenu_sel_bg: hex("#04395e"),
            wildmenu_sel_fg: hex("#ffffff"),

            command_bg: hex("#1e1e1e"),
            command_fg: hex("#d4d4d4"),

            line_number_fg: hex("#858585"),
            line_number_active_fg: hex("#c6c6c6"),

            separator: hex("#414141"),

            git_added: hex("#587c0c"),
            git_modified: hex("#0c7d9d"),
            git_deleted: hex("#94151b"),

            completion_bg: hex("#252526"),
            completion_selected_bg: hex("#04395e"),
            completion_fg: hex("#d4d4d4"),
            completion_border: hex("#454545"),

            diagnostic_error: hex("#f14c4c"),
            diagnostic_warning: hex("#cca700"),
            diagnostic_info: hex("#3794ff"),
            diagnostic_hint: hex("#858585"),
            spell_error: hex("#4fc1ff"),
            lightbulb: hex("#cca700"),

            hover_bg: hex("#252526"),
            hover_fg: hex("#d4d4d4"),
            hover_border: hex("#454545"),

            fuzzy_bg: hex("#252526"),
            fuzzy_selected_bg: hex("#04395e"),
            fuzzy_fg: hex("#d4d4d4"),
            fuzzy_query_fg: hex("#0097fb"),
            fuzzy_border: hex("#007acc"),
            fuzzy_title_fg: hex("#dcdcaa"),
            fuzzy_match_fg: hex("#0097fb"),

            // (bg #1e1e1e)
            diff_added_bg: hex("#14541a"),
            diff_removed_bg: hex("#541a1a"),
            diff_padding_bg: hex("#2d2d2d"),

            dap_stopped_bg: hex("#3a3000"),

            cursorline_bg: hex("#1e1e1e").cursorline_tint(), // derived from background

            yank_highlight_bg: hex("#dcdcaa"),
            yank_highlight_alpha: 0.25,

            annotation_fg: hex("#858585"),
            ghost_text_fg: hex("#5a5a5a"),

            md_heading1: hex("#dcdcaa"),
            md_heading2: hex("#569cd6"),
            md_heading3: hex("#c586c0"),
            md_code: hex("#ce9178"),
            md_link: hex("#3794ff"),

            sidebar_sel_bg: hex("#04395e"), // focused: visible blue highlight
            sidebar_sel_bg_inactive: hex("#2a2d2e"),
            semantic_parameter: hex("#9cdcfe"),   // light blue
            semantic_property: hex("#9cdcfe"),    // light blue
            semantic_namespace: hex("#4ec9b0"),   // teal
            semantic_enum_member: hex("#4fc1ff"), // bright blue
            semantic_interface: hex("#4ec9b0"),   // teal
            semantic_type_parameter: hex("#4ec9b0"),
            semantic_decorator: hex("#dcdcaa"), // yellow
            semantic_macro: hex("#dcdcaa"),     // yellow

            breadcrumb_bg: hex("#1e1e1e"),
            breadcrumb_fg: hex("#858585"),
            breadcrumb_active_fg: hex("#d4d4d4"),

            indent_guide_fg: hex("#404040"),
            indent_guide_active_fg: hex("#707070"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: hex("#3a3d41"),

            explorer_dir_fg: hex("#dcdcaa"), // warm yellow (like function names)
            explorer_file_fg: hex("#bbbbbb"), // VSCode default sidebar fg
            explorer_active_bg: hex("#2a2d3e"), // current-file tint

            scrollbar_thumb: hex("#5a5a5a"),
            scrollbar_track: hex("#1e1e1e"),
            terminal_bg: hex("#1e1e1e"),
            activity_bar_fg: hex("#c8c8d2"),
            activity_active_accent: hex("#c8c8d2"),
        }
    }

    /// VS Code Light+ (Default Light+) colour scheme.
    pub fn vscode_light() -> Self {
        let bg = hex("#ffffff");
        Self {
            background: bg,
            active_background: hex("#f3f3f3"),
            foreground: hex("#333333"),

            keyword: hex("#0000ff"),      // blue (storage)
            control_flow: hex("#af00db"), // purple (if, else, for, return)
            string_lit: hex("#a31515"),   // red
            comment: hex("#008000"),      // green
            function: hex("#795e26"),     // brown
            type_name: hex("#267f99"),    // teal
            variable: hex("#001080"),     // dark blue
            number: hex("#098658"),       // green
            operator: hex("#333333"),
            punctuation: hex("#333333"),
            macro_call: hex("#795e26"),
            attribute: hex("#267f99"),
            lifetime: hex("#0000ff"),
            constant: hex("#0070c1"),
            escape: hex("#ee0000"),
            boolean: hex("#0000ff"),
            property: hex("#001080"),
            parameter: hex("#001080"),
            module: hex("#267f99"),
            default_fg: hex("#333333"),

            selection: hex("#add6ff"),
            selection_alpha: 0.6,

            cursor: hex("#000000"),
            cursor_normal_alpha: 0.6,

            search_match_bg: hex("#e8be5a"),
            search_current_match_bg: hex("#a8ac94"),
            search_match_fg: hex("#000000"),

            tab_bar_bg: hex("#ececec"),
            tab_active_bg: hex("#ffffff"),
            tab_active_fg: hex("#333333"),
            tab_inactive_fg: hex("#8e8e8e"),
            tab_preview_active_fg: hex("#555555"),
            tab_preview_inactive_fg: hex("#999999"),
            tab_active_accent: hex("#005fb8"),

            status_bg: hex("#007acc"),
            status_fg: hex("#ffffff"),

            status_mode_normal_bg: hex("#007acc"),
            status_mode_insert_bg: hex("#16825d"),
            status_mode_visual_bg: hex("#68217a"),
            status_mode_replace_bg: hex("#c72e0f"),
            status_inactive_bg: hex("#e0e0e0"),
            status_inactive_fg: hex("#666666"),

            wildmenu_bg: hex("#f3f3f3"),
            wildmenu_fg: hex("#333333"),
            wildmenu_sel_bg: hex("#0060c0"),
            wildmenu_sel_fg: hex("#ffffff"),

            command_bg: hex("#ffffff"),
            command_fg: hex("#333333"),

            line_number_fg: hex("#237893"),
            line_number_active_fg: hex("#0b216f"),

            separator: hex("#d4d4d4"),

            git_added: hex("#48985e"),
            git_modified: hex("#2090d0"),
            git_deleted: hex("#e51400"),

            completion_bg: hex("#f3f3f3"),
            completion_selected_bg: hex("#0060c0"),
            completion_fg: hex("#333333"),
            completion_border: hex("#c8c8c8"),

            diagnostic_error: hex("#e51400"),
            diagnostic_warning: hex("#bf8803"),
            diagnostic_info: hex("#1a85ff"),
            diagnostic_hint: hex("#6c6c6c"),
            spell_error: hex("#1a85ff"),
            lightbulb: hex("#ddb100"),

            hover_bg: hex("#f3f3f3"),
            hover_fg: hex("#333333"),
            hover_border: hex("#c8c8c8"),

            fuzzy_bg: hex("#ffffff"),
            fuzzy_selected_bg: hex("#0060c0"),
            fuzzy_fg: hex("#333333"),
            fuzzy_query_fg: hex("#0066bf"),
            fuzzy_border: hex("#007acc"),
            fuzzy_title_fg: hex("#795e26"),
            fuzzy_match_fg: hex("#0066bf"),

            diff_added_bg: hex("#dfffdf"),
            diff_removed_bg: hex("#ffdede"),
            diff_padding_bg: hex("#f0f0f0"),

            dap_stopped_bg: hex("#ffffcc"),

            cursorline_bg: hex("#ffffff").cursorline_tint(), // derived from background

            yank_highlight_bg: hex("#795e26"),
            yank_highlight_alpha: 0.2,

            annotation_fg: hex("#8e8e8e"),
            ghost_text_fg: hex("#b0b0b0"),

            md_heading1: hex("#795e26"),
            md_heading2: hex("#0000ff"),
            md_heading3: hex("#af00db"),
            md_code: hex("#a31515"),
            md_link: hex("#0066bf"),

            sidebar_sel_bg: hex("#b4d9ff"), // focused: visible blue highlight
            sidebar_sel_bg_inactive: hex("#e4e6f1"),
            semantic_parameter: hex("#001080"),   // dark blue
            semantic_property: hex("#001080"),    // dark blue
            semantic_namespace: hex("#267f99"),   // teal
            semantic_enum_member: hex("#0070c1"), // blue
            semantic_interface: hex("#267f99"),   // teal
            semantic_type_parameter: hex("#267f99"),
            semantic_decorator: hex("#795e26"), // brown
            semantic_macro: hex("#795e26"),     // brown

            breadcrumb_bg: hex("#ffffff"),
            breadcrumb_fg: hex("#8e8e8e"),
            breadcrumb_active_fg: hex("#333333"),

            indent_guide_fg: hex("#d3d3d3"),
            indent_guide_active_fg: hex("#939393"),
            colorcolumn_bg: bg.colorcolumn_tint(),
            bracket_match_bg: hex("#dddddd"),

            explorer_dir_fg: hex("#795e26"),    // warm brown dirs
            explorer_file_fg: hex("#3b3b3b"),   // VSCode light sidebar fg
            explorer_active_bg: hex("#dce5f0"), // current-file tint

            scrollbar_thumb: hex("#b0b0b0"),
            scrollbar_track: hex("#f3f3f3"),
            terminal_bg: hex("#ffffff"),
            activity_bar_fg: hex("#646e6e"),
            activity_active_accent: hex("#646e6e"),
        }
    }

    /// Return a theme by name. Falls back to `onedark` for unknown names.
    pub fn from_name(name: &str) -> Self {
        match name {
            "gruvbox" | "gruvbox-dark" => Self::gruvbox_dark(),
            "tokyo-night" | "tokyonight" => Self::tokyo_night(),
            "solarized" | "solarized-dark" => Self::solarized_dark(),
            "vscode-dark" | "vscode" | "dark+" => Self::vscode_dark(),
            "vscode-light" | "light+" => Self::vscode_light(),
            "onedark" => Self::onedark(),
            _ => {
                // Try loading a VSCode theme from ~/.config/vimcode/themes/
                if let Some(theme) = Self::load_vscode_theme(name) {
                    theme
                } else {
                    Self::onedark()
                }
            }
        }
    }

    /// Returns `true` when the theme has a light background (relative luminance > 0.5).
    pub fn is_light(&self) -> bool {
        let (r, g, b) = (
            self.background.r as f64 / 255.0,
            self.background.g as f64 / 255.0,
            self.background.b as f64 / 255.0,
        );
        // Perceptual luminance (sRGB)
        0.299 * r + 0.587 * g + 0.114 * b > 0.5
    }

    /// Return the list of all built-in theme names.
    pub fn available_names() -> Vec<String> {
        let mut names: Vec<String> = vec![
            "onedark".into(),
            "gruvbox-dark".into(),
            "tokyo-night".into(),
            "solarized-dark".into(),
            "vscode-dark".into(),
            "vscode-light".into(),
        ];
        // Append custom VSCode themes from the platform config dir's themes/
        // subdirectory (see `core::paths::vimcode_config_dir`).
        if let Some(dir) = Self::themes_dir() {
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
        names
    }

    /// The directory where custom VSCode theme JSON files are stored.
    pub(crate) fn themes_dir() -> Option<std::path::PathBuf> {
        Some(crate::core::paths::vimcode_config_dir().join("themes"))
    }

    /// Try to load a VSCode-format `.json` theme file by name.
    /// Looks in `<vimcode_config_dir>/themes/<name>.json`
    /// (`~/.config/vimcode/themes/` on Linux/macOS, `%APPDATA%\vimcode\themes\`
    /// on Windows).
    pub fn load_vscode_theme(name: &str) -> Option<Self> {
        let dir = Self::themes_dir()?;
        let path = dir.join(format!("{name}.json"));
        Self::from_vscode_json(&path)
    }

    /// Parse a VSCode theme JSON file and map its colours to a `Theme`.
    /// Falls back to OneDark defaults for any missing keys.
    ///
    /// #829 note: this stays a local implementation rather than delegating
    /// the `colors` (chrome) mapping to `quadraui::Theme::from_vscode_json`
    /// (quadraui#775). That upstream function was "lifted from" this one
    /// but has since diverged for quadraui's own primitive set — e.g. it
    /// reads `title_fg` from `titleBar.activeForeground` and `header_bg`
    /// from `sideBarSectionHeader.background`, where vimcode derives the
    /// conceptually-similar `fuzzy_title_fg` from the syntax palette
    /// (`theme.type_name`) and `status_bg` from `statusBar.background`.
    /// A field-name-based reverse mapping from the upstream `quadraui::Theme`
    /// back onto this struct would silently change which VS Code JSON key
    /// feeds which vimcode field — exactly what "assert rendered chrome
    /// colours match before and after" (#829's acceptance bar) exists to
    /// catch. This `colors`-object walk stays local for that reason;
    /// only the `Color` type itself (this file's former `Color` struct,
    /// `to_q_color` / `to_quadraui_color` conversions) was adopted from
    /// quadraui, per #829. `strip_json_comments` itself has no
    /// theme-specific semantics — it moved to `quadraui::text_util` in
    /// #1494.
    pub fn from_vscode_json(path: &std::path::Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        // VSCode themes often have comments — strip them
        let data = quadraui::text_util::strip_json_comments(&data);
        let val: serde_json::Value = serde_json::from_str(&data).ok()?;
        let colors = val.get("colors");
        let token_colors = val.get("tokenColors");

        // Start from OneDark and override what the theme provides
        let mut theme = Self::onedark();

        // Helper: get a color from the "colors" object
        let color =
            |key: &str| -> Option<Color> { colors?.get(key)?.as_str().and_then(try_from_hex) };

        // ── Editor core ───────────────────────────────────────────────────
        if let Some(c) = color("editor.background") {
            theme.background = c;
            theme.active_background = c.lighten(0.02);
            theme.command_bg = c;
            theme.cursorline_bg = c.cursorline_tint();
        }
        if let Some(c) = color("editor.foreground") {
            theme.foreground = c;
            theme.default_fg = c;
            theme.command_fg = c;
        }

        // ── Selection / cursor ────────────────────────────────────────────
        if let Some(c) = color("editor.selectionBackground") {
            theme.selection = c;
        }
        if let Some(c) = color("editorCursor.foreground") {
            theme.cursor = c;
        }

        // ── Cursor line highlight ─────────────────────────────────────────
        if let Some(c) = color("editor.lineHighlightBackground") {
            theme.cursorline_bg = c;
        }
        if let Some(c) = color("editorRuler.foreground") {
            theme.colorcolumn_bg = c;
        }

        // ── Search ────────────────────────────────────────────────────────
        if let Some(c) = color("editor.findMatchBackground") {
            theme.search_current_match_bg = c;
        }
        if let Some(c) = color("editor.findMatchHighlightBackground") {
            theme.search_match_bg = c;
        }

        // ── Line numbers ──────────────────────────────────────────────────
        if let Some(c) = color("editorLineNumber.foreground") {
            theme.line_number_fg = c;
        }
        if let Some(c) = color("editorLineNumber.activeForeground") {
            theme.line_number_active_fg = c;
        }

        // ── Tab bar ───────────────────────────────────────────────────────
        if let Some(c) = color("editorGroupHeader.tabsBackground") {
            theme.tab_bar_bg = c;
        }
        if let Some(c) = color("tab.activeBackground") {
            theme.tab_active_bg = c;
        }
        if let Some(c) = color("tab.activeForeground") {
            theme.tab_active_fg = c;
        }
        if let Some(c) = color("tab.inactiveForeground") {
            theme.tab_inactive_fg = c;
            theme.tab_preview_inactive_fg = c.darken(0.3);
            theme.tab_preview_active_fg = c.lighten(0.2);
        }
        if let Some(c) = color("tab.activeBorderTop") {
            theme.tab_active_accent = c;
        }

        // ── Status bar ────────────────────────────────────────────────────
        if let Some(c) = color("statusBar.background") {
            theme.status_bg = c;
        }
        if let Some(c) = color("statusBar.foreground") {
            theme.status_fg = c;
        }

        // ── Wildmenu (derive from status bar) ─────────────────────────────
        if let Some(c) = color("statusBar.background") {
            theme.wildmenu_bg = c;
        }
        if let Some(c) = color("statusBar.foreground") {
            theme.wildmenu_fg = c;
        }

        // ── Separator ─────────────────────────────────────────────────────
        if let Some(c) = color("editorGroup.border") {
            theme.separator = c;
        }

        // ── Widgets (completion, hover, fuzzy) ────────────────────────────
        if let Some(c) = color("editorWidget.background") {
            theme.completion_bg = c;
            theme.hover_bg = c;
            theme.fuzzy_bg = c;
        }
        if let Some(c) = color("editorWidget.border") {
            theme.completion_border = c;
            theme.hover_border = c;
            theme.fuzzy_border = c;
        }
        if let Some(c) = color("editorSuggestWidget.selectedBackground") {
            theme.completion_selected_bg = c;
            theme.fuzzy_selected_bg = c;
        }
        if let Some(c) = color("editorWidget.foreground").or_else(|| color("editor.foreground")) {
            theme.completion_fg = c;
            theme.hover_fg = c;
            theme.fuzzy_fg = c;
        }

        // ── Sidebar ──────────────────────────────────────────────────────
        if let Some(c) = color("list.activeSelectionBackground") {
            theme.sidebar_sel_bg = c;
        }
        if let Some(c) = color("list.inactiveSelectionBackground") {
            theme.sidebar_sel_bg_inactive = c;
            theme.explorer_active_bg = c;
        }
        if let Some(c) = color("sideBar.foreground") {
            theme.explorer_file_fg = c;
        }

        // ── Scrollbar / terminal / activity bar ─────────────────────────
        if let Some(c) = color("scrollbarSlider.background") {
            theme.scrollbar_thumb = c;
            // VSCode doesn't have a separate track colour; derive from background
            theme.scrollbar_track = theme.background;
        }
        if let Some(c) = color("terminal.background") {
            theme.terminal_bg = c;
        }
        if let Some(c) = color("activityBar.foreground") {
            theme.activity_bar_fg = c;
        }
        // VS Code's own default for `activityBar.activeBorder` is
        // `activityBar.foreground` (see `ACTIVITY_BAR_ACTIVE_BORDER` in
        // VS Code's `colorRegistry`) — only fall back when the imported
        // theme doesn't override it explicitly.
        if let Some(c) =
            color("activityBar.activeBorder").or_else(|| color("activityBar.foreground"))
        {
            theme.activity_active_accent = c;
        }

        // ── Breadcrumbs ──────────────────────────────────────────────────
        if let Some(c) = color("breadcrumb.background") {
            theme.breadcrumb_bg = c;
        }
        if let Some(c) = color("breadcrumb.foreground") {
            theme.breadcrumb_fg = c;
        }
        if let Some(c) = color("breadcrumb.focusForeground")
            .or_else(|| color("breadcrumb.activeSelectionForeground"))
        {
            theme.breadcrumb_active_fg = c;
        }

        // ── Git gutter ────────────────────────────────────────────────────
        if let Some(c) = color("editorGutter.addedBackground")
            .or_else(|| color("gitDecoration.addedResourceForeground"))
        {
            theme.git_added = c;
        }
        if let Some(c) = color("editorGutter.modifiedBackground")
            .or_else(|| color("gitDecoration.modifiedResourceForeground"))
        {
            theme.git_modified = c;
        }
        if let Some(c) = color("editorGutter.deletedBackground")
            .or_else(|| color("gitDecoration.deletedResourceForeground"))
        {
            theme.git_deleted = c;
        }

        // ── Diagnostics ──────────────────────────────────────────────────
        if let Some(c) = color("editorError.foreground") {
            theme.diagnostic_error = c;
        }
        if let Some(c) = color("editorWarning.foreground") {
            theme.diagnostic_warning = c;
        }
        if let Some(c) = color("editorInfo.foreground") {
            theme.diagnostic_info = c;
        }
        if let Some(c) = color("editorHint.foreground") {
            theme.diagnostic_hint = c;
        }
        if let Some(c) = color("editorSpellChecker.foreground") {
            theme.spell_error = c;
        }

        // ── Diff ─────────────────────────────────────────────────────────
        // Alpha-blend diff backgrounds against the editor background so that
        // `#rrggbbaa` values (common in VSCode themes) produce correct results.
        if let Some(s) = colors
            .and_then(|c| c.get("diffEditor.insertedTextBackground"))
            .and_then(|v| v.as_str())
        {
            if let Some(c) = try_from_hex_over(s, theme.background) {
                theme.diff_added_bg = c;
            }
        }
        if let Some(s) = colors
            .and_then(|c| c.get("diffEditor.removedTextBackground"))
            .and_then(|v| v.as_str())
        {
            if let Some(c) = try_from_hex_over(s, theme.background) {
                theme.diff_removed_bg = c;
            }
        }

        // ── Annotations / ghost text ─────────────────────────────────────
        if let Some(c) = color("editorGhostText.foreground") {
            theme.ghost_text_fg = c;
        }

        // ── Token colours (syntax highlighting) ──────────────────────────
        if let Some(tc) = token_colors.and_then(|v| v.as_array()) {
            for entry in tc {
                let settings = match entry.get("settings") {
                    Some(s) => s,
                    None => continue,
                };
                let fg = settings
                    .get("foreground")
                    .and_then(|v| v.as_str())
                    .and_then(try_from_hex);
                let fg = match fg {
                    Some(c) => c,
                    None => continue,
                };
                let scopes = match entry.get("scope") {
                    Some(serde_json::Value::String(s)) => {
                        s.split(',').map(|s| s.trim()).collect::<Vec<_>>()
                    }
                    Some(serde_json::Value::Array(arr)) => {
                        arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                    }
                    _ => continue,
                };
                for scope in &scopes {
                    match *scope {
                        "keyword" | "storage" | "storage.type" | "storage.modifier" => {
                            theme.keyword = fg;
                        }
                        "keyword.control"
                        | "keyword.control.flow"
                        | "keyword.control.conditional"
                        | "keyword.control.loop"
                        | "keyword.control.trycatch"
                        | "keyword.control.import" => {
                            theme.control_flow = fg;
                        }
                        "string"
                        | "string.quoted"
                        | "string.quoted.double"
                        | "string.quoted.single" => {
                            theme.string_lit = fg;
                        }
                        "comment" | "comment.line" | "comment.block" => {
                            theme.comment = fg;
                            theme.annotation_fg = fg;
                        }
                        "entity.name.function" | "support.function" | "meta.function-call" => {
                            theme.function = fg;
                        }
                        "entity.name.type"
                        | "support.type"
                        | "support.class"
                        | "entity.name.class"
                        | "entity.name.type.class" => {
                            theme.type_name = fg;
                            theme.semantic_namespace = fg;
                            theme.semantic_interface = fg;
                            theme.semantic_type_parameter = fg;
                        }
                        "variable" | "variable.other" | "variable.language" => {
                            theme.variable = fg;
                        }
                        "constant.numeric"
                        | "constant.numeric.integer"
                        | "constant.numeric.float" => {
                            theme.number = fg;
                        }
                        "entity.name.tag" => {
                            theme.semantic_decorator = fg;
                        }
                        "variable.parameter" | "variable.parameter.function" => {
                            theme.semantic_parameter = fg;
                        }
                        "variable.other.property" | "support.type.property-name" => {
                            theme.semantic_property = fg;
                        }
                        "variable.other.enummember" | "constant.other.enum" => {
                            theme.semantic_enum_member = fg;
                        }
                        "entity.name.function.macro" | "support.function.macro" => {
                            theme.semantic_macro = fg;
                            theme.macro_call = fg;
                        }
                        "keyword.operator"
                        | "keyword.operator.expression"
                        | "keyword.operator.logical" => {
                            theme.operator = fg;
                        }
                        "punctuation"
                        | "punctuation.definition"
                        | "punctuation.bracket"
                        | "punctuation.separator" => {
                            theme.punctuation = fg;
                        }
                        "entity.other.attribute-name" | "meta.attribute" => {
                            theme.attribute = fg;
                        }
                        "storage.modifier.lifetime" | "punctuation.definition.lifetime" => {
                            theme.lifetime = fg;
                        }
                        "constant" | "constant.language" | "constant.other" => {
                            theme.constant = fg;
                            theme.boolean = fg;
                        }
                        "constant.character.escape" => {
                            theme.escape = fg;
                        }
                        "entity.name.namespace" | "entity.name.module" => {
                            theme.module = fg;
                        }
                        _ => {}
                    }
                }
            }
        }

        // ── Derive remaining colours from the base palette ───────────────
        // Fuzzy finder query/title inherit from syntax colours if not set
        theme.fuzzy_query_fg = theme.function;
        theme.fuzzy_title_fg = theme.type_name;

        // Markdown headings from syntax palette
        theme.md_heading1 = theme.type_name;
        theme.md_heading2 = theme.function;
        theme.md_heading3 = theme.keyword;
        theme.md_code = theme.string_lit;
        theme.md_link = theme.function;

        Some(theme)
    }

    /// Return the foreground colour for a Tree-sitter scope name.
    pub fn scope_color(&self, scope: &str) -> Color {
        self.scope_color_opt(scope).unwrap_or(self.default_fg)
    }

    /// Same scope-name -> colour mapping as [`scope_color`](Self::scope_color),
    /// but `None` for a name this theme doesn't recognise instead of
    /// silently falling back to `default_fg`. Lets a caller (e.g. #1653's
    /// `vimcode.decor.set_hl` `link` resolution in `resolve_decor_style`)
    /// tell "this is a real theme role" apart from "unknown name, use the
    /// default foreground" — `scope_color` can't make that distinction once
    /// it's collapsed to a concrete `Color`.
    pub fn scope_color_opt(&self, scope: &str) -> Option<Color> {
        Some(match scope {
            "keyword" => self.keyword,
            "keyword.control" => self.control_flow,
            "operator" => self.operator,
            "string" => self.string_lit,
            "comment" => self.comment,
            "function" | "function.call" | "method" | "method.call" => self.function,
            "type" | "class" | "struct" | "enum" | "interface" => self.type_name,
            "variable" => self.variable,
            "number" => self.number,
            "boolean" => self.boolean,
            "constant" => self.constant,
            "punctuation"
            | "punctuation.bracket"
            | "punctuation.delimiter"
            | "punctuation.special" => self.punctuation,
            "macro" | "macro_call" => self.macro_call,
            "attribute" => self.attribute,
            "lifetime" => self.lifetime,
            "escape" => self.escape,
            "module" | "namespace" => self.module,
            "parameter" => self.parameter,
            "property" | "field" => self.property,
            _ => return None,
        })
    }

    /// Map an LSP semantic token type + modifiers to a style.
    /// Returns `None` for unknown/unmapped token types (preserves tree-sitter coloring).
    pub fn semantic_token_style(&self, token_type: &str, modifiers: &[String]) -> Option<Style> {
        let fg = match token_type {
            "parameter" => self.semantic_parameter,
            "property" => self.semantic_property,
            "namespace" => self.semantic_namespace,
            "enumMember" => self.semantic_enum_member,
            "interface" => self.semantic_interface,
            "typeParameter" => self.semantic_type_parameter,
            "decorator" => self.semantic_decorator,
            "macro" => self.semantic_macro,
            // Reuse existing syntax colors for standard token types
            "keyword" | "modifier" => {
                // rust-analyzer sends "controlFlow" modifier for if/else/for/while/return etc.
                if modifiers.iter().any(|m| m == "controlFlow") {
                    self.control_flow
                } else {
                    self.keyword
                }
            }
            "function" | "method" => self.function,
            "type" | "class" | "struct" | "enum" => self.type_name,
            "variable" => self.variable,
            "string" | "regexp" => self.string_lit,
            "comment" => self.comment,
            "number" => self.number,
            "operator" => self.operator,
            "boolean" => self.boolean,
            "lifetime" => self.lifetime,
            "attribute" | "attributeBracket" => self.attribute,
            "builtinType" => self.type_name,
            _ => return None,
        };
        let bold = modifiers
            .iter()
            .any(|m| m == "declaration" || m == "definition");
        let italic = modifiers
            .iter()
            .any(|m| m == "readonly" || m == "static" || m == "deprecated");
        Some(Style {
            fg,
            bg: None,
            bold,
            italic,
            font_scale: 1.0,
        })
    }
}

/// Whether a per-window status line is reserved (and painted) at the bottom
/// of each editor window's own rect, given the engine's current state.
///
/// Single source of truth for "does this window reserve its bottom row for
/// its own status line" (#728). Before this, the question was answered
/// independently — and inconsistently — in two places:
///   - `build_screen_layout_with_breadcrumb_row` used `per_window_status &&
///     !separate_status`, correctly handling `status_line_above_terminal`
///     being OFF with the bottom panel open (which pulls the active
///     window's status into a *separated* bar above the terminal instead,
///     freeing that window's own bottom row) but never checking
///     `terminal_maximized`.
///   - GTK's old hand-rolled h-scrollbar geometry helper used
///     `window_status_line && !terminal_maximized` (to avoid offsetting the
///     horizontal scrollbar for a status row that isn't painted while the
///     terminal panel covers the editor windows entirely), but never
///     checked `separate_status`.
///
/// Each covered an axis the other didn't, so either one alone could
/// disagree with what actually gets painted.
///
/// #1128: GTK's own scrollbar geometry no longer consults this function at
/// all — `quadraui::Editor::layout` (what paint actually uses, quadraui#968)
/// lays scrollbars out against the window's raw, unshrunk rect regardless of
/// a per-window status line, so `app_support::editor_scrollbar_layout`
/// applying an offset here would just reintroduce a hover/paint disagreement
/// in the opposite direction. `build_screen_layout_with_breadcrumb_row`
/// remains this function's one live caller.
pub fn window_status_row_reserved(engine: &Engine) -> bool {
    // While the terminal panel is maximized, editor windows are not the
    // visible surface at all (`breadcrumb_draw_targets` suppresses every
    // breadcrumb the same way), so nothing paints a per-window status row
    // regardless of the setting.
    if engine.terminal_maximized {
        return false;
    }
    let per_window_status = effective_window_status_line(engine);
    let bottom_panel_open = engine.terminal_open || engine.bottom_panel_open;
    let separate_status =
        per_window_status && !engine.settings.status_line_above_terminal && bottom_panel_open;
    per_window_status && !separate_status
}

/// `Settings::window_status_line` narrowed by `'laststatus'` (#1206, `:h
/// 'laststatus'`) — the value every status-line-visibility read site should
/// use instead of the raw field.
///
/// `'laststatus'` is `0`/`1`/`2`/`3` in real Vim; vimcode's status line is
/// architecturally per-window (`Settings::window_status_line`), not the
/// single split-spanning line real Vim's `3` draws, so `3` falls back to
/// `2`'s behavior here rather than being modeled — see the field doc
/// comment on [`crate::core::Settings::laststatus`].
pub fn effective_window_status_line(engine: &Engine) -> bool {
    match engine.settings.laststatus {
        0 => false,
        1 => engine.settings.window_status_line && engine.windows.len() > 1,
        _ => engine.settings.window_status_line,
    }
}

/// Whether `'laststatus'` allows *any* status line — per-window or global —
/// to be visible at all, independent of `Settings::window_status_line`
/// (`:h 'laststatus'`).
///
/// This is the other half of `'laststatus'`'s policy that
/// `effective_window_status_line` alone cannot express: that function
/// answers "should each window paint its own row", so it also returns
/// `false` for `laststatus=0` and `laststatus=1` with one window — the two
/// cases where no status line of *any* kind should show. A naive
/// `!effective_window_status_line(engine)` read of that `false` as "show
/// the single global bar instead" (the correct reading for
/// `window_status_line=false` at `laststatus=2`) reintroduces exactly the
/// row it was supposed to hide (#1235 follow-up: `laststatus=0` painted a
/// full-width global status bar with the per-window-row-count row freed by
/// this same fix, because `global_status_bar`'s `if per_window_status {
/// None } else { Some(..) }` conflated "not per-window" with "show the
/// global fallback").
pub fn any_status_line_visible(engine: &Engine) -> bool {
    match engine.settings.laststatus {
        0 => false,
        1 => engine.windows.len() > 1,
        _ => true,
    }
}

/// Whether the bottom band needs a dedicated row for the **global**
/// (non-per-window) status bar — i.e. no window paints its own status row
/// (`!effective_window_status_line`) but `'laststatus'` still allows some
/// status line to show (`any_status_line_visible`). When this is `false`,
/// the bottom band's status-line footprint is a single row: either each
/// window carries its own (`effective_window_status_line` true), or
/// `'laststatus'` hides the status line entirely and only the always-present
/// command line remains.
pub fn global_status_bar_visible(engine: &Engine) -> bool {
    !effective_window_status_line(engine) && any_status_line_visible(engine)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ─── quadraui theme mapping (#1574) ────────────────────────────────────
    //
    // `to_quadraui_theme` (and its `_chrome`/`_editor` halves) is the single
    // place vimcode's rich `render::Theme` is narrowed down to the small
    // `quadraui::Theme` every rasteriser actually paints with. Before #1574,
    // three of its fields were wrong or missing entirely and fell through to
    // quadraui's own hardcoded dark `Theme::default()` — this is the
    // regression test for that class of bug, not just the two named fields
    // the issue reported.

    /// The two concrete bugs #1574 reported: `inactive_selected_bg` was
    /// never set at all (falling through to quadraui's dark
    /// `Theme::default()`, `rgb(35, 40, 58)`, painted behind `#333` text —
    /// unreadable), and `scrollbar_track` was mapped to `theme.separator`
    /// instead of vimcode's own dedicated `theme.scrollbar_track` field.
    /// Both are asserted against `vscode_light`, whose values for these
    /// fields are nothing like quadraui's dark defaults, so a regression
    /// back to either bug fails loudly here instead of only looking wrong
    /// on screen.
    #[test]
    fn to_quadraui_theme_maps_inactive_selected_bg_and_scrollbar_track_from_vimcode_theme() {
        let theme = Theme::vscode_light();
        let q = to_quadraui_theme(&theme);
        let default = quadraui::Theme::default();

        assert_eq!(q.inactive_selected_bg, theme.sidebar_sel_bg_inactive);
        assert_ne!(q.inactive_selected_bg, default.inactive_selected_bg);

        assert_eq!(q.scrollbar_track, theme.scrollbar_track);
        assert_ne!(q.scrollbar_track, default.scrollbar_track);
    }

    /// The remaining unmapped keys #1574 audited (`badge_*`, `board_*`,
    /// `card_hint_*`) — every one must trace back to a vimcode theme field
    /// rather than silently keeping quadraui's dark default, the same way
    /// `inactive_selected_bg` did before this issue.
    #[test]
    fn to_quadraui_theme_maps_badge_board_and_card_hint_keys_from_vimcode_theme() {
        let theme = Theme::vscode_light();
        let q = to_quadraui_theme(&theme);
        let default = quadraui::Theme::default();

        assert_eq!(q.board_selected_card_bg, theme.fuzzy_selected_bg);
        assert_eq!(q.board_col_header_bg, theme.status_bg);
        assert_eq!(q.badge_running, theme.lightbulb);
        assert_eq!(q.badge_passed, theme.git_added);
        assert_eq!(q.badge_warning, theme.diagnostic_warning);
        assert_eq!(q.badge_blocked, theme.diagnostic_error);
        assert_eq!(q.card_hint_bg, theme.hover_bg);
        assert_eq!(q.card_hint_fg, theme.hover_fg);

        // None of these six keys' `vscode_light` values happen to coincide
        // with quadraui's own dark default — if they ever silently fell
        // back to it (e.g. a future field addition landing behind a
        // reintroduced `..Theme::default()` spread), this would catch it.
        assert_ne!(q.board_selected_card_bg, default.board_selected_card_bg);
        assert_ne!(q.board_col_header_bg, default.board_col_header_bg);
        assert_ne!(q.badge_running, default.badge_running);
        assert_ne!(q.badge_passed, default.badge_passed);
        assert_ne!(q.badge_warning, default.badge_warning);
        assert_ne!(q.badge_blocked, default.badge_blocked);
        assert_ne!(q.card_hint_bg, default.card_hint_bg);
        assert_ne!(q.card_hint_fg, default.card_hint_fg);
    }

    /// Every field on `quadraui::Theme` must be reachable from
    /// `to_quadraui_theme`'s output via *some* vimcode `Theme` field — this
    /// is the structural half of #1574: `to_quadraui_theme_chrome`'s struct
    /// literal has no `..quadraui::Theme::default()` spread any more, so a
    /// future field added upstream is a compile error here (`E0063`) until
    /// it's mapped, rather than a silent dark leak discovered by eye later.
    /// This test pins the full mapping table so a regression on any single
    /// field (not just the ones called out above) is caught.
    #[test]
    fn to_quadraui_theme_maps_every_field_from_vscode_light() {
        let theme = Theme::vscode_light();
        let q = to_quadraui_theme(&theme);

        assert_eq!(q.background, theme.background);
        assert_eq!(q.foreground, theme.foreground);
        assert_eq!(q.tab_bar_bg, theme.tab_bar_bg);
        assert_eq!(q.tab_active_bg, theme.tab_active_bg);
        assert_eq!(q.tab_active_fg, theme.tab_active_fg);
        assert_eq!(q.tab_inactive_fg, theme.tab_inactive_fg);
        assert_eq!(q.tab_preview_active_fg, theme.tab_preview_active_fg);
        assert_eq!(q.tab_preview_inactive_fg, theme.tab_preview_inactive_fg);
        assert_eq!(q.separator, theme.separator);
        assert_eq!(q.surface_bg, theme.fuzzy_bg);
        assert_eq!(q.surface_fg, theme.fuzzy_fg);
        assert_eq!(q.selected_bg, theme.fuzzy_selected_bg);
        assert_eq!(q.inactive_selected_bg, theme.sidebar_sel_bg_inactive);
        assert_eq!(q.border_fg, theme.fuzzy_border);
        assert_eq!(q.title_fg, theme.fuzzy_title_fg);
        assert_eq!(q.header_bg, theme.status_bg);
        assert_eq!(q.header_fg, theme.status_fg);
        assert_eq!(q.muted_fg, theme.line_number_fg);
        assert_eq!(q.error_fg, theme.diagnostic_error);
        assert_eq!(q.warning_fg, theme.diagnostic_warning);
        assert_eq!(q.query_fg, theme.fuzzy_query_fg);
        assert_eq!(q.match_fg, theme.fuzzy_match_fg);
        assert_eq!(q.accent_fg, theme.cursor);
        assert_eq!(q.hover_bg, theme.hover_bg);
        assert_eq!(q.hover_fg, theme.hover_fg);
        assert_eq!(q.hover_border, theme.hover_border);
        assert_eq!(q.input_bg, theme.completion_bg);
        assert_eq!(q.inactive_fg, theme.status_inactive_fg);
        assert_eq!(q.selection_bg, theme.selection);
        assert_eq!(q.link_fg, theme.md_link);
        assert_eq!(q.completion_bg, theme.completion_bg);
        assert_eq!(q.completion_fg, theme.completion_fg);
        assert_eq!(q.completion_border, theme.completion_border);
        assert_eq!(q.completion_selected_bg, theme.completion_selected_bg);
        assert_eq!(q.accent_bg, theme.tab_active_accent);
        assert_eq!(q.scrollbar_track, theme.scrollbar_track);
        assert_eq!(q.scrollbar_thumb, theme.scrollbar_thumb);
        assert_eq!(q.command_line_bg, theme.command_bg);
        assert_eq!(q.command_line_fg, theme.command_fg);
        assert_eq!(q.editor_active_background, theme.active_background);
        assert_eq!(q.cursorline_bg, theme.cursorline_bg);
        assert_eq!(q.dap_stopped_bg, theme.dap_stopped_bg);
        assert_eq!(q.colorcolumn_bg, theme.colorcolumn_bg);
        assert_eq!(q.diff_added_bg, theme.diff_added_bg);
        assert_eq!(q.diff_removed_bg, theme.diff_removed_bg);
        assert_eq!(q.diff_padding_bg, theme.diff_padding_bg);
        assert_eq!(q.line_number_fg, theme.line_number_fg);
        assert_eq!(q.line_number_active_fg, theme.line_number_active_fg);
        assert_eq!(q.diagnostic_error, theme.diagnostic_error);
        assert_eq!(q.diagnostic_warning, theme.diagnostic_warning);
        assert_eq!(q.diagnostic_info, theme.diagnostic_info);
        assert_eq!(q.diagnostic_hint, theme.diagnostic_hint);
        assert_eq!(q.git_added, theme.git_added);
        assert_eq!(q.git_modified, theme.git_modified);
        assert_eq!(q.git_deleted, theme.git_deleted);
        assert_eq!(q.lightbulb, theme.lightbulb);
        assert_eq!(q.spell_error, theme.spell_error);
        assert_eq!(q.cursor, theme.cursor);
        assert_eq!(q.cursor_normal_alpha, theme.cursor_normal_alpha as f32);
        assert_eq!(q.selection, theme.selection);
        assert_eq!(q.selection_alpha, theme.selection_alpha as f32);
        assert_eq!(q.yank_highlight_bg, theme.yank_highlight_bg);
        assert_eq!(q.yank_highlight_alpha, theme.yank_highlight_alpha as f32);
        assert_eq!(q.bracket_match_bg, theme.bracket_match_bg);
        assert_eq!(q.indent_guide_fg, theme.indent_guide_fg);
        assert_eq!(q.indent_guide_active_fg, theme.indent_guide_active_fg);
        assert_eq!(q.annotation_fg, theme.annotation_fg);
        assert_eq!(q.ghost_text_fg, theme.ghost_text_fg);
        assert_eq!(q.board_selected_card_bg, theme.fuzzy_selected_bg);
        assert_eq!(q.board_col_header_bg, theme.status_bg);
        assert_eq!(q.badge_running, theme.lightbulb);
        assert_eq!(q.badge_passed, theme.git_added);
        assert_eq!(q.badge_warning, theme.diagnostic_warning);
        assert_eq!(q.badge_blocked, theme.diagnostic_error);
        assert_eq!(q.card_hint_bg, theme.hover_bg);
        assert_eq!(q.card_hint_fg, theme.hover_fg);
    }
}
