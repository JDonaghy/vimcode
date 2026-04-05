use super::*;

/// Sentinel path stored in the dummy placeholder child of unexpanded directories.
pub(super) const TREE_DUMMY_PATH: &str = "__vimcode_loading__";

/// Build file tree with a root folder node at the top (like VSCode).
/// Only the root's immediate children are populated; subdirectories are
/// lazily expanded via the `row-expanded` signal (see `tree_row_expanded`).
pub(super) fn build_file_tree_with_root(
    store: &gtk4::TreeStore,
    root: &Path,
    show_hidden: bool,
    case_insensitive: bool,
    dir_fg_hex: &str,
    file_fg_hex: &str,
) {
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string())
        .to_uppercase();
    let root_iter = store.insert_with_values(
        None,
        None,
        &[
            (0, &""),
            (1, &root_name),
            (2, &root.to_string_lossy().to_string()),
            (3, &file_fg_hex),
            (4, &""),
            (5, &file_fg_hex),
        ],
    );
    build_file_tree_shallow(
        store,
        Some(&root_iter),
        root,
        show_hidden,
        case_insensitive,
        dir_fg_hex,
        file_fg_hex,
    );
}

/// Populate one level of children under `parent`.  For each child directory
/// a dummy placeholder row is added so the expand arrow appears, but its
/// contents are not read until the user actually expands the row.
pub(super) fn build_file_tree_shallow(
    store: &gtk4::TreeStore,
    parent: Option<&gtk4::TreeIter>,
    path: &Path,
    show_hidden: bool,
    case_insensitive: bool,
    dir_fg_hex: &str,
    file_fg_hex: &str,
) {
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();

    // Sort: directories first, then files, both alphabetically
    entries.sort_by(|a, b| {
        let a_is_dir = a.path().is_dir();
        let b_is_dir = b.path().is_dir();
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                if case_insensitive {
                    let an = a.file_name().to_string_lossy().to_lowercase();
                    let bn = b.file_name().to_string_lossy().to_lowercase();
                    an.cmp(&bn)
                } else {
                    a.file_name().cmp(&b.file_name())
                }
            }
        }
    });

    for entry in entries {
        let child_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') && name != "." && name != ".." && !show_hidden {
            continue;
        }

        let is_dir = child_path.is_dir();
        let ext = child_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let icon = if is_dir {
            crate::icons::FOLDER.nerd
        } else {
            crate::icons::file_icon(ext)
        };

        let fg_hex: &str = file_fg_hex;
        let iter = store.insert_with_values(
            parent,
            None,
            &[
                (0, &icon),
                (1, &name),
                (2, &child_path.to_string_lossy().to_string()),
                (3, &fg_hex),
                (4, &""),
                (5, &dir_fg_hex),
            ],
        );

        // For directories, insert a dummy child so the expand arrow appears.
        if is_dir {
            store.insert_with_values(
                Some(&iter),
                None,
                &[
                    (0, &""),
                    (1, &""),
                    (2, &TREE_DUMMY_PATH),
                    (3, &dir_fg_hex),
                    (4, &""),
                    (5, &dir_fg_hex),
                ],
            );
        }
    }
}

/// Called when a tree row is expanded.  Replaces the dummy placeholder with
/// the directory's real contents (one level deep).
pub(super) fn tree_row_expanded(
    store: &gtk4::TreeStore,
    iter: &gtk4::TreeIter,
    show_hidden: bool,
    case_insensitive: bool,
    dir_fg_hex: &str,
    file_fg_hex: &str,
) {
    use gtk4::prelude::TreeModelExt;
    let dir_path: String = store.get_value(iter, 2).get().unwrap_or_default();
    if dir_path.is_empty() {
        return;
    }

    // Check whether the first child is the dummy placeholder.
    if let Some(child) = store.iter_children(Some(iter)) {
        let child_path: String = store.get_value(&child, 2).get().unwrap_or_default();
        if child_path == TREE_DUMMY_PATH {
            // Populate real children BEFORE removing the dummy so the
            // directory never has zero children — GTK auto-collapses a
            // row the instant its last child is removed, which caused
            // the "first click swallowed" bug.
            build_file_tree_shallow(
                store,
                Some(iter),
                Path::new(&dir_path),
                show_hidden,
                case_insensitive,
                dir_fg_hex,
                file_fg_hex,
            );
            store.remove(&child);
        }
        // If the first child is NOT the dummy, the directory was already
        // populated (e.g. collapsed and re-expanded) — nothing to do.
    }
}

/// Walk the entire TreeStore and update columns 4 (indicator text) and 5
/// (indicator color) based on git status and LSP diagnostics.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_tree_indicators(
    store: &gtk4::TreeStore,
    git_statuses: &std::collections::HashMap<PathBuf, char>,
    diag_counts: &std::collections::HashMap<PathBuf, (usize, usize)>,
    added_color: &str,
    modified_color: &str,
    deleted_color: &str,
    error_color: &str,
    warning_color: &str,
    default_fg: &str,
) {
    use gtk4::prelude::TreeModelExt;
    #[allow(clippy::too_many_arguments)]
    fn walk(
        store: &gtk4::TreeStore,
        parent: Option<&gtk4::TreeIter>,
        git_statuses: &std::collections::HashMap<PathBuf, char>,
        diag_counts: &std::collections::HashMap<PathBuf, (usize, usize)>,
        added_color: &str,
        modified_color: &str,
        deleted_color: &str,
        error_color: &str,
        warning_color: &str,
        default_fg: &str,
    ) {
        let Some(iter) = store.iter_children(parent) else {
            return;
        };
        loop {
            let path_str: String = store.get_value(&iter, 2).get().unwrap_or_default();
            if !path_str.is_empty()
                && path_str != TREE_DUMMY_PATH
                && !path_str.starts_with("__NEW_FILE__")
                && !path_str.starts_with("__NEW_FOLDER__")
            {
                let p = PathBuf::from(&path_str);
                let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
                let git_label = git_statuses.get(&canon).copied();
                let (errors, warnings) = diag_counts.get(&canon).copied().unwrap_or((0, 0));

                if git_label.is_some() || errors > 0 || warnings > 0 {
                    let mut parts = Vec::new();
                    // Diagnostics first (like VSCode), then git status
                    let mut color = modified_color;
                    if errors > 0 {
                        let s = if errors > 9 {
                            "9+".to_string()
                        } else {
                            format!("{errors}")
                        };
                        parts.push(s);
                        color = error_color;
                    }
                    if warnings > 0 {
                        let s = if warnings > 9 {
                            "9+".to_string()
                        } else {
                            format!("{warnings}")
                        };
                        parts.push(s);
                        if errors == 0 {
                            color = warning_color;
                        }
                    }
                    if let Some(label) = git_label {
                        parts.push(label.to_string());
                        if errors == 0 && warnings == 0 {
                            color = match label {
                                'A' | '?' => added_color,
                                'D' => deleted_color,
                                _ => modified_color,
                            };
                        }
                    }
                    let text = parts.join(" ");
                    store.set_value(&iter, 4, &text.into());
                    store.set_value(&iter, 5, &color.into());
                    // Set name foreground (column 3) to match the indicator color.
                    store.set_value(&iter, 3, &color.into());
                } else {
                    store.set_value(&iter, 4, &"".into());
                    // Use a valid color to avoid GTK "Don't know color ''" warnings.
                    store.set_value(&iter, 5, &modified_color.into());
                    // Reset name color to default.
                    store.set_value(&iter, 3, &default_fg.into());
                }
            }
            // Recurse into children
            walk(
                store,
                Some(&iter),
                git_statuses,
                diag_counts,
                added_color,
                modified_color,
                deleted_color,
                error_color,
                warning_color,
                default_fg,
            );
            if !store.iter_next(&iter) {
                break;
            }
        }
    }
    walk(
        store,
        None,
        git_statuses,
        diag_counts,
        added_color,
        modified_color,
        deleted_color,
        error_color,
        warning_color,
        default_fg,
    );
}

/// Get the parent directory for creating a new file/folder, based on the
/// currently selected tree row. If a directory is selected, use it. If a
/// file is selected, use its parent. Fallback: cwd.
pub(super) fn selected_parent_dir(tv: &gtk4::TreeView) -> PathBuf {
    if let Some((model, iter)) = tv.selection().selected() {
        if let Ok(s) = model.get_value(&iter, 2).get::<String>() {
            if !s.is_empty() {
                let p = PathBuf::from(s);
                if p.is_dir() {
                    return p;
                }
                if let Some(parent) = p.parent() {
                    return parent.to_path_buf();
                }
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Like `selected_parent_dir` but takes the `Rc<RefCell<Option<TreeView>>>` used by `App`.
pub(super) fn selected_parent_dir_from_app(
    tv_ref: &std::rc::Rc<std::cell::RefCell<Option<gtk4::TreeView>>>,
) -> PathBuf {
    if let Some(ref tv) = *tv_ref.borrow() {
        return selected_parent_dir(tv);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Get the full path of the currently selected tree row (from App's Rc<RefCell>).
pub(super) fn selected_file_path_from_app(
    tv_ref: &std::rc::Rc<std::cell::RefCell<Option<gtk4::TreeView>>>,
) -> Option<PathBuf> {
    if let Some(ref tv) = *tv_ref.borrow() {
        if let Some((model, iter)) = tv.selection().selected() {
            if let Ok(s) = model.get_value(&iter, 2).get::<String>() {
                if !s.is_empty() {
                    return Some(PathBuf::from(s));
                }
            }
        }
    }
    None
}

/// Validate filename for file/folder creation
pub(super) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name cannot be empty".to_string());
    }

    if name.contains('/') || name.contains('\\') {
        return Err("Name cannot contain slashes".to_string());
    }

    if name.contains('\0') {
        return Err("Name cannot contain null characters".to_string());
    }

    // Platform-specific invalid characters
    #[cfg(windows)]
    {
        if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
            return Err("Name contains invalid characters".to_string());
        }
    }

    // Reserved names
    if name == "." || name == ".." {
        return Err("Invalid name".to_string());
    }

    Ok(())
}

/// Find and select file in tree, expanding parents if needed.
/// With lazy loading, parent directories may not be populated yet, so we
/// walk the path components from the root, expanding (and thus populating)
/// each ancestor before searching for the next child.
pub(super) fn highlight_file_in_tree(tree_view: &gtk4::TreeView, file_path: &Path) {
    let Some(model) = tree_view.model() else {
        return;
    };
    let Some(tree_store) = model.downcast_ref::<gtk4::TreeStore>() else {
        return;
    };

    // Find the cwd root node (first child of the store).
    let Some(root_iter) = tree_store.iter_first() else {
        return;
    };
    let root_path_str: String = tree_store
        .get_value(&root_iter, 2)
        .get()
        .unwrap_or_default();
    let root_path = PathBuf::from(&root_path_str);
    let rel = match file_path.strip_prefix(&root_path) {
        Ok(r) => r,
        Err(_) => return, // file not under the project root
    };

    // Walk the relative path components, expanding each directory.
    let mut current_iter = root_iter;
    for component in rel.components() {
        let name = component.as_os_str().to_string_lossy();

        // Ensure this directory's children are populated (trigger lazy load).
        let tp = tree_store.path(&current_iter);
        tree_view.expand_row(&tp, false);

        // Search children for the matching name.
        let mut found = false;
        if let Some(child_iter) = tree_store.iter_children(Some(&current_iter)) {
            loop {
                let child_name: String = tree_store
                    .get_value(&child_iter, 1)
                    .get()
                    .unwrap_or_default();
                if child_name == name.as_ref() {
                    current_iter = child_iter;
                    found = true;
                    break;
                }
                if !tree_store.iter_next(&child_iter) {
                    break;
                }
            }
        }
        if !found {
            return;
        }
    }

    // current_iter now points to the target file/directory.
    let tree_path = tree_store.path(&current_iter);

    // Expand parents so the row is visible.
    if tree_path.depth() > 1 {
        let mut parent_path = tree_path.clone();
        parent_path.up();
        tree_view.expand_to_path(&parent_path);
    }

    tree_view.selection().select_path(&tree_path);
    tree_view.scroll_to_cell(
        Some(&tree_path),
        None::<&gtk4::TreeViewColumn>,
        false,
        0.0,
        0.0,
    );
}

/// Find a TreeStore iter whose column 2 (path) matches the given filesystem path.
/// Searches the entire tree recursively.  Returns `None` if not found.
pub(super) fn find_tree_iter_for_path(
    store: &gtk4::TreeStore,
    target: &Path,
) -> Option<gtk4::TreeIter> {
    let target_str = target.to_string_lossy();
    let iter = store.iter_first()?;
    find_iter_recursive(store, &iter, &target_str)
}

fn find_iter_recursive(
    store: &gtk4::TreeStore,
    iter: &gtk4::TreeIter,
    target: &str,
) -> Option<gtk4::TreeIter> {
    loop {
        let path_str: String = store.get_value(iter, 2).get().unwrap_or_default();
        if path_str == target {
            return Some(*iter);
        }
        // Recurse into children
        if let Some(child) = store.iter_children(Some(iter)) {
            if let Some(found) = find_iter_recursive(store, &child, target) {
                return Some(found);
            }
        }
        if !store.iter_next(iter) {
            break;
        }
    }
    None
}

/// Recursively remove any rows with `__NEW_FILE__` or `__NEW_FOLDER__` markers
/// in column 2.  Called when inline editing is cancelled.
pub(super) fn remove_new_entry_rows(store: &gtk4::TreeStore, iter: &gtk4::TreeIter) {
    loop {
        let path_str: String = store.get_value(iter, 2).get().unwrap_or_default();
        if path_str.starts_with("__NEW_FILE__") || path_str.starts_with("__NEW_FOLDER__") {
            if !store.remove(iter) {
                return; // no more siblings
            }
            continue; // re-check at same position (remove shifts the next row in)
        }
        // Recurse into children
        if let Some(child) = store.iter_children(Some(iter)) {
            remove_new_entry_rows(store, &child);
        }
        if !store.iter_next(iter) {
            break;
        }
    }
}
