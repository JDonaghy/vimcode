use super::*;

/// Populate the `SidebarSystem` on `engine.sc_sidebar_system` with current
/// row data for all 5 SC sections. Call once per frame before
/// `sidebar_system.render()` or `.handle_cached()`.
pub fn populate_sc_sidebar_system(engine: &Engine, theme: &Theme) {
    use crate::core::engine::{
        SC_SECTION_CHANGES, SC_SECTION_LOG, SC_SECTION_MERGE, SC_SECTION_STAGED,
        SC_SECTION_WORKTREES,
    };
    use quadraui::{Decoration, StyledSpan, StyledText, TreeRow};

    let add_fg = theme.git_added;
    let del_fg = theme.git_deleted;
    let mod_fg = theme.git_modified;
    let dim_fg = theme.status_inactive_fg;

    let merge = engine.sc_section_files(SC_SECTION_MERGE);
    let staged = engine.sc_section_files(SC_SECTION_STAGED);
    let unstaged = engine.sc_section_files(SC_SECTION_CHANGES);
    let show_worktrees = engine.sc_worktrees.len() > 1;
    // #991: the Merge Changes section is only shown when the tree actually
    // has a conflict, so a clean repo still paints exactly two file
    // sections (the "not always-on" half of this issue).
    let show_merge = !merge.is_empty();

    let file_row = |i: usize, f: &crate::core::git::FileStatus, section: usize| {
        let kind = match section {
            // Conflict rows carry the '!' marker, VS Code's conflict char.
            SC_SECTION_MERGE => Some(crate::core::git::StatusKind::Unmerged),
            SC_SECTION_STAGED => f.staged,
            _ => f.unstaged,
        };
        let ch = kind.map(|k| k.label()).unwrap_or('?');
        let color = match ch {
            'A' => add_fg,
            // #1051: VS Code paints Untracked the same green family as
            // Added — distinct from Modified's orange/yellow. Falling
            // through to `mod_fg` here (the pre-#1051 behavior, back when
            // this arm was still '?') left the badge miscolored even after
            // the letter itself was corrected.
            'U' => add_fg,
            'D' => del_fg,
            '!' => del_fg,
            _ => mod_fg,
        };
        plain_tree_row(
            vec![i as u16],
            0,
            StyledText {
                spans: vec![
                    StyledSpan::with_fg(ch.to_string(), color),
                    StyledSpan::plain(format!(" {}", f.path)),
                ],
            },
        )
    };

    let merge_rows: Vec<TreeRow> = merge
        .iter()
        .enumerate()
        .map(|(i, f)| file_row(i, f, SC_SECTION_MERGE))
        .collect();

    let staged_rows: Vec<TreeRow> = staged
        .iter()
        .enumerate()
        .map(|(i, f)| file_row(i, f, SC_SECTION_STAGED))
        .collect();

    let unstaged_rows: Vec<TreeRow> = unstaged
        .iter()
        .enumerate()
        .map(|(i, f)| file_row(i, f, SC_SECTION_CHANGES))
        .collect();

    let worktree_rows: Vec<TreeRow> = engine
        .sc_worktrees
        .iter()
        .enumerate()
        .map(|(i, wt)| {
            let check = if wt.is_current { "\u{2713} " } else { "  " };
            let branch = wt.branch.as_deref().unwrap_or("HEAD");
            let main_marker = if wt.is_main { " [main]" } else { "" };
            let text = format!("{}{} {}{}", check, branch, wt.path.display(), main_marker);
            plain_tree_row(vec![i as u16], 0, StyledText::plain(text))
        })
        .collect();

    let log_rows: Vec<TreeRow> = engine
        .sc_log
        .iter()
        .enumerate()
        .map(|(i, entry)| TreeRow {
            path: vec![i as u16],
            indent: 0,
            icon: None,
            text: StyledText {
                spans: vec![
                    StyledSpan::with_fg(entry.hash.clone(), dim_fg),
                    StyledSpan::plain(format!(" {}", entry.message)),
                ],
            },
            badge: None,
            is_expanded: None,
            decoration: Decoration::Muted,
            edit: None,
        })
        .collect();

    let mut sidebar = engine.sc_sidebar_system.borrow_mut();
    sidebar.set_has_focus(engine.sc_has_focus);
    if engine.sc_has_focus && sidebar.active_section().is_none() {
        // Never land on the (possibly hidden) Merge Changes section by
        // default — its actions resolve conflicts (#991).
        sidebar.set_active_section(Some(SC_SECTION_STAGED));
    }

    let badge = |n: usize| {
        if n > 0 {
            Some(StyledText::plain(format!("({})", n)))
        } else {
            None
        }
    };
    sidebar.set_section_badge(SC_SECTION_MERGE, badge(merge.len()));
    sidebar.set_section_badge(SC_SECTION_STAGED, badge(staged.len()));
    sidebar.set_section_badge(SC_SECTION_CHANGES, badge(unstaged.len()));
    sidebar.set_section_badge(SC_SECTION_WORKTREES, badge(engine.sc_worktrees.len()));
    sidebar.set_section_badge(SC_SECTION_LOG, badge(engine.sc_log.len()));
    sidebar.set_section_visible(SC_SECTION_MERGE, show_merge);
    sidebar.set_section_visible(SC_SECTION_WORKTREES, show_worktrees);

    sidebar.set_rows(SC_SECTION_MERGE, merge_rows);
    sidebar.set_rows(SC_SECTION_STAGED, staged_rows);
    sidebar.set_rows(SC_SECTION_CHANGES, unstaged_rows);
    sidebar.set_rows(SC_SECTION_WORKTREES, worktree_rows);
    sidebar.set_rows(SC_SECTION_LOG, log_rows);
}

/// Populate the Search panel's `SidebarSystem` with current form + tree
/// data. Section 0 is the Form (query/replace/toggles/buttons/status);
/// Section 1 is the TreeView (results grouped by file). Call once per
/// frame before `sidebar.render()`.
pub fn populate_search_sidebar_system(engine: &Engine, root: &std::path::Path, theme: &Theme) {
    use quadraui::primitives::form::{ButtonRowItem, FieldKind, ToggleGroupItem};
    use quadraui::{Badge, Decoration, Form, FormField, StyledSpan, StyledText, TreeRow, WidgetId};

    let opts = &engine.project_search_options;
    let results = &engine.project_search_results;

    // ── Section 0: Form (search chrome) ─────────────────────────────────
    let form_focus = engine.search_panel_form_focus.borrow();
    let query_focused = form_focus.as_deref() == Some("search:query");
    let replace_focused = form_focus.as_deref() == Some("search:replace");

    let form = Form {
        id: WidgetId::new("search-form"),
        fields: vec![
            FormField {
                id: WidgetId::new("search:query"),
                label: StyledText::default(),
                kind: FieldKind::TextInput {
                    value: engine.project_search_query.clone(),
                    placeholder: "Search…".to_string(),
                    cursor: if query_focused {
                        Some(engine.search_query_caret.get())
                    } else {
                        None
                    },
                    selection_anchor: None,
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:replace"),
                label: StyledText::default(),
                kind: FieldKind::TextInput {
                    value: engine.project_replace_text.clone(),
                    placeholder: "Replace…".to_string(),
                    cursor: if replace_focused {
                        Some(engine.replace_text_caret.get())
                    } else {
                        None
                    },
                    selection_anchor: None,
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:toggles"),
                label: StyledText::default(),
                kind: FieldKind::ToggleGroup {
                    toggles: vec![
                        ToggleGroupItem {
                            id: WidgetId::new("search:case"),
                            label: "Aa".to_string(),
                            value: opts.case_sensitive,
                        },
                        ToggleGroupItem {
                            id: WidgetId::new("search:word"),
                            label: "Ab|".to_string(),
                            value: opts.whole_word,
                        },
                        ToggleGroupItem {
                            id: WidgetId::new("search:regex"),
                            label: ".*".to_string(),
                            value: opts.use_regex,
                        },
                    ],
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:buttons"),
                label: StyledText::default(),
                kind: FieldKind::ButtonRow {
                    buttons: vec![
                        ButtonRowItem {
                            id: WidgetId::new("search:find_next"),
                            label: "Find".to_string(),
                            disabled: engine.project_search_query.is_empty(),
                            icon: None,
                        },
                        ButtonRowItem {
                            id: WidgetId::new("search:replace_next"),
                            label: "Repl".to_string(),
                            disabled: results.is_empty(),
                            icon: None,
                        },
                        ButtonRowItem {
                            id: WidgetId::new("search:replace_all"),
                            label: "All".to_string(),
                            disabled: results.is_empty(),
                            icon: None,
                        },
                    ],
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
            FormField {
                id: WidgetId::new("search:status"),
                label: StyledText::default(),
                kind: FieldKind::ReadOnly {
                    value: StyledText::plain(if results.is_empty() {
                        if engine.project_search_query.is_empty() {
                            "Type to search, Enter to run".to_string()
                        } else if engine.project_search_status.is_empty() {
                            String::new()
                        } else {
                            engine.project_search_status.clone()
                        }
                    } else {
                        engine.project_search_status.clone()
                    }),
                },
                hint: StyledText::default(),
                disabled: false,
                validation: None,
            },
        ],
        focused_field: form_focus.as_deref().map(WidgetId::new),
        scroll_offset: 0,
        has_focus: query_focused || replace_focused,
    };

    // ── Section 1: TreeView (results grouped by file) ───────────────────
    let collapsed = engine.search_collapsed_files.borrow();
    let mut tree_rows: Vec<TreeRow> = Vec::new();
    let mut file_idx: usize = 0;
    let mut last_file: Option<&std::path::Path> = None;
    let mut match_within_file: usize = 0;
    let mut file_match_count: usize = 0;

    for m in results.iter() {
        if last_file != Some(m.file.as_path()) {
            if let Some(prev_header) = tree_rows.iter_mut().rev().find(|r| r.path.len() == 1) {
                prev_header.badge = Some(Badge::plain(format!("({})", file_match_count)));
            }
            if last_file.is_some() {
                file_idx += 1;
            }
            last_file = Some(m.file.as_path());
            match_within_file = 0;
            file_match_count = 0;

            let expanded = !collapsed.contains(&file_idx);
            let rel = m.file.strip_prefix(root).unwrap_or(&m.file);
            tree_rows.push(TreeRow {
                path: vec![file_idx as u16],
                indent: 0,
                icon: None,
                text: StyledText {
                    spans: vec![StyledSpan::plain(rel.display().to_string())],
                },
                badge: None,
                is_expanded: Some(expanded),
                decoration: Decoration::Header,
                edit: None,
            });
        }

        let expanded = !collapsed.contains(&file_idx);
        if expanded {
            let line_prefix = format!("{:>4}: ", m.line + 1);
            tree_rows.push(plain_tree_row(
                vec![file_idx as u16, match_within_file as u16],
                1,
                StyledText {
                    spans: vec![
                        StyledSpan {
                            text: line_prefix,
                            // #1574: was a hard-coded `rgb(100, 100, 100)`
                            // literal — unreadable on light colourschemes.
                            // Use the same muted tone the gutter's own line
                            // numbers paint with.
                            fg: Some(theme.line_number_fg),
                            bg: None,
                            bold: false,
                            italic: false,
                            underline: false,
                        },
                        StyledSpan::plain(m.line_text.trim().to_string()),
                    ],
                },
            ));
        }
        match_within_file += 1;
        file_match_count += 1;
    }
    if let Some(prev_header) = tree_rows.iter_mut().rev().find(|r| r.path.len() == 1) {
        prev_header.badge = Some(Badge::plain(format!("({})", file_match_count)));
    }

    let mut sidebar = engine.search_sidebar_system.borrow_mut();
    sidebar.set_has_focus(engine.search_has_focus);
    sidebar.set_form(0, form);
    sidebar.set_rows(1, tree_rows);

    if engine.search_has_focus && sidebar.active_section().is_none() {
        sidebar.set_active_section(Some(0));
    }
}

/// Populate the `TreeController` on `engine.explorer_tree` with current
/// row data. Call once per frame before `tree_controller.render()` or
/// `.handle()`.
pub fn populate_explorer_tree_controller(engine: &Engine, theme: &Theme) {
    let mut tree = engine.explorer_tree.borrow_mut();
    tree.set_has_focus(engine.explorer_has_focus);
    let tree_rows = build_explorer_tree_rows(&engine.explorer_rows, engine, theme);
    tree.set_rows(tree_rows);
}

pub(crate) fn build_explorer_tree_rows(
    rows: &[ExplorerRow],
    engine: &Engine,
    theme: &Theme,
) -> Vec<quadraui::TreeRow> {
    use quadraui::{Badge, Decoration, Icon as QIcon, StyledText, TreeRow};

    let (git_statuses, diag_counts) = engine.explorer_indicators();
    let err_fg = theme.diagnostic_error;
    let warn_fg = theme.diagnostic_warning;

    let mut out: Vec<TreeRow> = Vec::with_capacity(rows.len());
    for (row_idx, row) in rows.iter().enumerate() {
        let canon = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());

        let diag = diag_counts.get(&canon).copied();
        let git_label = git_statuses.get(&canon).copied();

        let decoration = match diag {
            Some((e, _)) if e > 0 => Decoration::Error,
            Some((_, w)) if w > 0 => Decoration::Warning,
            _ if git_label.is_some() => Decoration::Modified,
            _ => Decoration::Normal,
        };

        let badge = if let Some((errors, warnings)) = diag {
            if errors > 0 {
                Some(Badge::colored(
                    if errors > 9 {
                        "9+".to_string()
                    } else {
                        errors.to_string()
                    },
                    err_fg,
                ))
            } else if warnings > 0 {
                Some(Badge::colored(
                    if warnings > 9 {
                        "9+".to_string()
                    } else {
                        warnings.to_string()
                    },
                    warn_fg,
                ))
            } else {
                git_label.map(|label| Badge::plain(label.to_string()))
            }
        } else {
            git_label.map(|label| Badge::plain(label.to_string()))
        };

        let icon = if row.is_dir {
            Some(QIcon::new(
                icons::FOLDER.nerd.to_string(),
                icons::FOLDER.fallback.to_string(),
            ))
        } else {
            // #992: filename-matched first (`Dockerfile`, `.gitignore`, ...
            // -- `Path::extension()` is `None` for both a no-dot filename
            // and a leading-dot dotfile, so an extension-only lookup can
            // never badge either), falling back to the extension table.
            let glyph = icons::file_icon_for_name(&row.name).to_string();
            let file_icon = QIcon::new(glyph, ".".to_string());
            // #1381: match the tab bar's filetype colour (`build_tab_bar_icons`
            // / `tab_icon_color`) so the same file gets the same glyph colour
            // in both places. Gated on Nerd Fonts the same way the tab bar
            // gates its icons entirely (`build_tab_bar_icons` returns `&[]`
            // when disabled): with Nerd Fonts off, leave the icon uncoloured
            // rather than tinting the plain ASCII fallback glyph.
            let file_icon = if icons::nerd_fonts_enabled() {
                file_icon.with_color(tab_icon_color(&row.name))
            } else {
                file_icon
            };
            Some(file_icon)
        };

        out.push(TreeRow {
            path: vec![row_idx as u16],
            indent: row.depth as u16,
            icon,
            text: StyledText::plain(&row.name),
            badge,
            is_expanded: if row.is_dir {
                Some(row.is_expanded)
            } else {
                None
            },
            decoration,
            edit: None,
        });
    }
    out
}

fn empty_placeholder_row(session_active: bool) -> quadraui::TreeRow {
    use quadraui::{Decoration, StyledText, TreeRow};
    let hint = if session_active {
        "(empty)"
    } else {
        "(not running)"
    };
    TreeRow {
        path: vec![u16::MAX],
        indent: 0,
        icon: None,
        text: StyledText::plain(hint.to_string()),
        badge: None,
        is_expanded: None,
        decoration: Decoration::Muted,
        edit: None,
    }
}

pub(crate) fn build_dap_var_rows(engine: &Engine, session_active: bool) -> Vec<quadraui::TreeRow> {
    use quadraui::{StyledText, TreeRow};

    let mut rows: Vec<TreeRow> = Vec::new();
    let mut flat_idx: u16 = 0;

    fn push_var_tree(
        rows: &mut Vec<TreeRow>,
        vars: &[crate::core::dap::DapVariable],
        depth: u16,
        flat_idx: &mut u16,
        expanded: &std::collections::HashSet<u64>,
        children_map: &std::collections::HashMap<u64, Vec<crate::core::dap::DapVariable>>,
    ) {
        for v in vars {
            let prefix = if v.var_ref > 0 {
                if expanded.contains(&v.var_ref) {
                    icons::EXPAND_DOWN.nerd
                } else {
                    icons::COLLAPSE_RIGHT.nerd
                }
            } else {
                "  "
            };
            let text = if v.value.is_empty() {
                format!("{}{}", prefix, v.name)
            } else {
                format!("{}{} = {}", prefix, v.name, v.value)
            };
            rows.push(plain_tree_row(
                vec![*flat_idx],
                depth,
                StyledText::plain(text),
            ));
            *flat_idx += 1;
            if v.var_ref > 0 && expanded.contains(&v.var_ref) {
                if let Some(child_vars) = children_map.get(&v.var_ref) {
                    push_var_tree(
                        rows,
                        child_vars,
                        depth + 1,
                        flat_idx,
                        expanded,
                        children_map,
                    );
                }
            }
        }
    }

    if engine.dap_primary_scope_ref > 0 {
        let expanded = engine
            .dap_expanded_vars
            .contains(&engine.dap_primary_scope_ref);
        let prefix = if expanded {
            icons::EXPAND_DOWN.nerd
        } else {
            icons::COLLAPSE_RIGHT.nerd
        };
        rows.push(plain_tree_row(
            vec![flat_idx],
            0,
            StyledText::plain(format!("{prefix}{}", engine.dap_primary_scope_name)),
        ));
        flat_idx += 1;
        if expanded {
            push_var_tree(
                &mut rows,
                &engine.dap_variables,
                1,
                &mut flat_idx,
                &engine.dap_expanded_vars,
                &engine.dap_child_variables,
            );
        }
    } else {
        push_var_tree(
            &mut rows,
            &engine.dap_variables,
            0,
            &mut flat_idx,
            &engine.dap_expanded_vars,
            &engine.dap_child_variables,
        );
    }

    for (scope_name, var_ref) in &engine.dap_scope_groups {
        let expanded = engine.dap_expanded_vars.contains(var_ref);
        let prefix = if expanded {
            icons::EXPAND_DOWN.nerd
        } else {
            icons::COLLAPSE_RIGHT.nerd
        };
        rows.push(plain_tree_row(
            vec![flat_idx],
            0,
            StyledText::plain(format!("{prefix}{scope_name}")),
        ));
        flat_idx += 1;
        if expanded {
            if let Some(child_vars) = engine.dap_child_variables.get(var_ref) {
                push_var_tree(
                    &mut rows,
                    child_vars,
                    1,
                    &mut flat_idx,
                    &engine.dap_expanded_vars,
                    &engine.dap_child_variables,
                );
            }
        }
    }

    if rows.is_empty() {
        vec![empty_placeholder_row(session_active)]
    } else {
        rows
    }
}

pub(crate) fn build_dap_watch_rows(
    engine: &Engine,
    session_active: bool,
) -> Vec<quadraui::TreeRow> {
    use quadraui::StyledText;

    if engine.dap_watch_expressions.is_empty() {
        return vec![empty_placeholder_row(session_active)];
    }

    engine
        .dap_watch_expressions
        .iter()
        .zip(engine.dap_watch_values.iter())
        .enumerate()
        .map(|(i, (expr, val))| {
            let val_str = val.as_deref().unwrap_or(if session_active {
                "\u{2026}" // …
            } else {
                "(not running)"
            });
            plain_tree_row(
                vec![i as u16],
                0,
                StyledText::plain(format!("{expr} = {val_str}")),
            )
        })
        .collect()
}

pub(crate) fn build_dap_stack_rows(
    engine: &Engine,
    session_active: bool,
) -> Vec<quadraui::TreeRow> {
    use quadraui::StyledText;

    if engine.dap_stack_frames.is_empty() {
        return vec![empty_placeholder_row(session_active)];
    }

    engine
        .dap_stack_frames
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let src = f
                .source
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("?");
            let prefix = if i == engine.dap_active_frame {
                icons::COLLAPSE_RIGHT.nerd
            } else {
                "  "
            };
            plain_tree_row(
                vec![i as u16],
                0,
                StyledText::plain(format!("{}{} ({}:{})", prefix, f.name, src, f.line)),
            )
        })
        .collect()
}

pub(crate) fn build_dap_bp_rows(engine: &Engine, session_active: bool) -> Vec<quadraui::TreeRow> {
    use quadraui::StyledText;

    let mut sorted_bp: Vec<_> = engine.dap_breakpoints.iter().collect();
    sorted_bp.sort_by_key(|(path, _)| path.as_str());

    let mut rows = Vec::new();
    let mut flat_idx: u16 = 0;
    for (path, bps) in &sorted_bp {
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        for bp in *bps {
            let suffix = if let Some(cond) = &bp.condition {
                format!(" [if {cond}]")
            } else if let Some(hc) = &bp.hit_condition {
                format!(" [hits {hc}]")
            } else if let Some(msg) = &bp.log_message {
                format!(" [log: {msg}]")
            } else {
                String::new()
            };
            let symbol = if bp.condition.is_some() || bp.hit_condition.is_some() {
                "\u{25c6}" // ◆ conditional
            } else {
                icons::DBG_BREAKPOINTS.nerd
            };
            rows.push(plain_tree_row(
                vec![flat_idx],
                0,
                StyledText::plain(format!("{} {}:{}{}", symbol, file_name, bp.line, suffix)),
            ));
            flat_idx += 1;
        }
    }

    if rows.is_empty() {
        vec![empty_placeholder_row(session_active)]
    } else {
        rows
    }
}
