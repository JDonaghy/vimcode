mod common;
use common::*;
use vimcode_core::quadraui;
use vimcode_core::RegType;

// ── Test helpers ─────────────────────────────────────────────────────────────

fn test_manifests() -> Vec<vimcode_core::core::extensions::ExtensionManifest> {
    use vimcode_core::core::extensions::*;
    vec![
        ExtensionManifest {
            name: "bash".to_string(),
            display_name: "Bash / Shell Support".to_string(),
            file_extensions: vec![".sh".to_string(), ".bash".to_string()],
            language_ids: vec!["shellscript".to_string(), "bash".to_string()],
            lsp: LspConfig {
                binary: "bash-language-server".to_string(),
                install: "npm install -g bash-language-server".to_string(),
                args: vec!["start".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ExtensionManifest {
            name: "cpp".to_string(),
            display_name: "C / C++ Language Support".to_string(),
            file_extensions: vec![".c".to_string(), ".h".to_string(), ".cpp".to_string()],
            language_ids: vec!["c".to_string(), "cpp".to_string()],
            lsp: LspConfig {
                binary: "clangd".to_string(),
                install_linux: "sudo apt-get install -y clangd".to_string(),
                install_macos: "brew install llvm".to_string(),
                install_windows: "winget install LLVM.LLVM".to_string(),
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "codelldb".to_string(),
                binary: "codelldb".to_string(),
                transport: "tcp".to_string(),
                args: vec!["--port".to_string(), "0".to_string()],
                ..Default::default()
            },
            workspace_markers: vec!["CMakeLists.txt".to_string()],
            ..Default::default()
        },
        ExtensionManifest {
            name: "csharp".to_string(),
            display_name: "C# Language Support".to_string(),
            file_extensions: vec![".cs".to_string(), ".csproj".to_string(), ".sln".to_string()],
            language_ids: vec!["csharp".to_string()],
            lsp: LspConfig {
                binary: "csharp-ls".to_string(),
                install: "dotnet tool install -g csharp-ls".to_string(),
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "netcoredbg".to_string(),
                binary: "netcoredbg".to_string(),
                transport: "stdio".to_string(),
                args: vec!["--interpreter=vscode".to_string()],
                ..Default::default()
            },
            ..Default::default()
        },
        ExtensionManifest {
            name: "git-insights".to_string(),
            display_name: "Git Insights".to_string(),
            scripts: vec![
                "blame.lua".to_string(),
                "history.lua".to_string(),
                "show.lua".to_string(),
                "line_history.lua".to_string(),
                "diff.lua".to_string(),
                "stash.lua".to_string(),
                "repo_log.lua".to_string(),
                "git_log_panel.lua".to_string(),
            ],
            ..Default::default()
        },
        ExtensionManifest {
            name: "go".to_string(),
            display_name: "Go Language Support".to_string(),
            file_extensions: vec![".go".to_string()],
            language_ids: vec!["go".to_string()],
            lsp: LspConfig {
                binary: "gopls".to_string(),
                install: "go install golang.org/x/tools/gopls@latest".to_string(),
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "delve".to_string(),
                binary: "dlv".to_string(),
                install: "go install github.com/go-delve/delve/cmd/dlv@latest".to_string(),
                transport: "stdio".to_string(),
                args: vec!["dap".to_string()],
                ..Default::default()
            },
            workspace_markers: vec!["go.mod".to_string(), "go.sum".to_string()],
            ..Default::default()
        },
        ExtensionManifest {
            name: "java".to_string(),
            display_name: "Java Language Support".to_string(),
            file_extensions: vec![".java".to_string()],
            language_ids: vec!["java".to_string()],
            lsp: LspConfig {
                binary: "jdtls".to_string(),
                // #918: jdtls previously had no install command on any
                // platform, so a user without it got a silent "No LSP
                // server found" with no indication java was even
                // involved. `brew install jdtls` is a real Homebrew-core
                // formula (confirmed via `brew info jdtls`) — fill in
                // macOS here. Linux/Windows are deliberately left empty:
                // neither has a comparable one-line package-manager
                // install for eclipse-jdtls, so filling them in is
                // deferred rather than shipping a bad command.
                install_macos: "brew install jdtls".to_string(),
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "java-debug".to_string(),
                binary: "java-debug-adapter".to_string(),
                // Deliberately left without an install command: the
                // java-debug adapter is a jar built from the
                // microsoft/java-debug sources with no portable one-line
                // install on any platform (see
                // `dap_manager::install_cmd_for_adapter`'s existing
                // "requires complex multi-step builds" fallback for
                // java-debug/js-debug). Defect 2's fix in
                // `ensure_server_for_language` means an empty LSP install
                // command still surfaces a named, actionable error instead
                // of a silent dead end; this DAP side is out of this
                // issue's file scope (`dap_manager.rs`).
                ..Default::default()
            },
            workspace_markers: vec!["pom.xml".to_string()],
            ..Default::default()
        },
        ExtensionManifest {
            name: "javascript".to_string(),
            display_name: "JavaScript / TypeScript Support".to_string(),
            file_extensions: vec![
                ".js".to_string(),
                ".jsx".to_string(),
                ".ts".to_string(),
                ".tsx".to_string(),
            ],
            language_ids: vec![
                "javascript".to_string(),
                "typescript".to_string(),
                "javascriptreact".to_string(),
                "typescriptreact".to_string(),
            ],
            lsp: LspConfig {
                binary: "typescript-language-server".to_string(),
                install: "npm install -g typescript typescript-language-server".to_string(),
                args: vec!["--stdio".to_string()],
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "js-debug".to_string(),
                binary: "node".to_string(),
                transport: "stdio".to_string(),
                ..Default::default()
            },
            workspace_markers: vec!["package.json".to_string(), "tsconfig.json".to_string()],
            ..Default::default()
        },
        ExtensionManifest {
            name: "php".to_string(),
            display_name: "PHP Language Support".to_string(),
            file_extensions: vec![".php".to_string()],
            language_ids: vec!["php".to_string()],
            lsp: LspConfig {
                binary: "intelephense".to_string(),
                install: "npm install -g intelephense".to_string(),
                ..Default::default()
            },
            ..Default::default()
        },
        ExtensionManifest {
            name: "python".to_string(),
            display_name: "Python Language Support".to_string(),
            file_extensions: vec![".py".to_string(), ".pyi".to_string(), ".pyw".to_string()],
            language_ids: vec!["python".to_string()],
            lsp: LspConfig {
                binary: "pyright-langserver".to_string(),
                install: "npm install -g pyright".to_string(),
                fallback_binaries: vec![
                    "basedpyright-langserver".to_string(),
                    "pylsp".to_string(),
                    "jedi-language-server".to_string(),
                ],
                args: vec!["--stdio".to_string()],
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "debugpy".to_string(),
                binary: "python".to_string(),
                transport: "stdio".to_string(),
                args: vec!["-m".to_string(), "debugpy.adapter".to_string()],
                ..Default::default()
            },
            workspace_markers: vec!["pyproject.toml".to_string(), "setup.py".to_string()],
            ..Default::default()
        },
        ExtensionManifest {
            name: "ruby".to_string(),
            display_name: "Ruby Language Support".to_string(),
            file_extensions: vec![
                ".rb".to_string(),
                ".rake".to_string(),
                ".gemspec".to_string(),
            ],
            language_ids: vec!["ruby".to_string()],
            lsp: LspConfig {
                binary: "ruby-lsp".to_string(),
                install: "gem install ruby-lsp".to_string(),
                ..Default::default()
            },
            workspace_markers: vec!["Gemfile".to_string()],
            ..Default::default()
        },
        ExtensionManifest {
            name: "rust".to_string(),
            display_name: "Rust Language Support".to_string(),
            file_extensions: vec![".rs".to_string()],
            language_ids: vec!["rust".to_string()],
            lsp: LspConfig {
                binary: "rust-analyzer".to_string(),
                install: "cargo install rust-analyzer".to_string(),
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "codelldb".to_string(),
                binary: "codelldb".to_string(),
                transport: "tcp".to_string(),
                args: vec!["--port".to_string(), "0".to_string()],
                ..Default::default()
            },
            workspace_markers: vec!["Cargo.toml".to_string()],
            ..Default::default()
        },
    ]
}

fn engine_with_registry(text: &str) -> vimcode_core::Engine {
    let mut e = engine_with(text);
    e.ext_registry = Some(test_manifests());
    e
}

fn ext_sidebar_setup(e: &mut vimcode_core::Engine, section: usize, idx: usize) {
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_body_rect
        .set(quadraui::Rect::new(0.0, 0.0, 200.0, 400.0));
    e.ext_sidebar_system.borrow_mut().set_backend_info(
        16.0,
        quadraui::MsvLayoutMetrics {
            header_size: 16.0,
            divider_size: 1.0,
            scrollbar_size: 8.0,
            cell_quantum: 0.0,
        },
    );
    e.populate_ext_sidebar_system();
    e.ext_sidebar_system.borrow_mut().set_has_focus(true);
    e.ext_sidebar_system
        .borrow_mut()
        .set_active_section(Some(section));
    e.ext_sidebar_system
        .borrow_mut()
        .set_selected_path(section, Some(vec![idx as u16]));
}

// ── :ExtList ──────────────────────────────────────────────────────────────────

#[test]
fn ext_list_shows_all_extensions() {
    let mut e = engine_with_registry("");
    exec(&mut e, "ExtList");
    // Message should mention several known extension names
    let msg = e.message.to_lowercase();
    assert!(
        msg.contains("csharp"),
        "ExtList should mention csharp: {msg}"
    );
    assert!(
        msg.contains("python"),
        "ExtList should mention python: {msg}"
    );
    assert!(msg.contains("rust"), "ExtList should mention rust: {msg}");
}

#[test]
fn ext_list_shows_installed_tag_after_install_tracking() {
    let mut e = engine_with_registry("");
    // Directly mark extension as installed in state (bypass actual LSP install)
    e.extension_state.mark_installed("csharp");
    exec(&mut e, "ExtList");
    let msg = e.message.to_lowercase();
    assert!(
        msg.contains("installed") || msg.contains("csharp"),
        "ExtList should acknowledge installed csharp: {msg}"
    );
}

// ── :ExtDisable / :ExtEnable ──────────────────────────────────────────────────

#[test]
fn ext_disable_marks_extension_as_dismissed() {
    let mut e = engine_with("");
    assert!(
        !e.extension_state.is_dismissed("csharp"),
        "csharp should not be dismissed initially"
    );
    exec(&mut e, "ExtDisable csharp");
    assert!(
        e.extension_state.is_dismissed("csharp"),
        "csharp should be dismissed after :ExtDisable"
    );
}

#[test]
fn ext_enable_removes_dismissed_status() {
    let mut e = engine_with("");
    e.extension_state.mark_dismissed("csharp");
    assert!(e.extension_state.is_dismissed("csharp"));

    exec(&mut e, "ExtEnable csharp");
    assert!(
        !e.extension_state.is_dismissed("csharp"),
        "csharp should no longer be dismissed after :ExtEnable"
    );
}

#[test]
fn ext_disable_does_not_affect_other_extensions() {
    let mut e = engine_with("");
    exec(&mut e, "ExtDisable csharp");
    assert!(!e.extension_state.is_dismissed("python"));
    assert!(!e.extension_state.is_dismissed("rust"));
}

#[test]
fn ext_install_marks_extension_installed() {
    let mut e = engine_with("");
    // Force-mark as installed (real install would shell out)
    e.extension_state.mark_installed("python");
    assert!(e.extension_state.is_installed("python"));
    assert!(!e.extension_state.is_installed("csharp"));
}

#[test]
fn ext_install_clears_dismissed_flag() {
    let mut e = engine_with("");
    e.extension_state.mark_dismissed("python");
    assert!(e.extension_state.is_dismissed("python"));
    e.extension_state.mark_installed("python");
    // mark_installed should remove from dismissed
    assert!(
        !e.extension_state.is_dismissed("python"),
        "installed extension should no longer be dismissed"
    );
}

// ── Line annotations (virtual text) ───────────────────────────────────────────

#[test]
fn line_annotations_can_be_set_and_read() {
    let mut e = engine_with("line one\nline two\n");
    assert!(e.line_annotations.is_empty());

    e.line_annotations
        .insert(0, "  Author • 2 days ago • fix bug".to_string());
    e.line_annotations
        .insert(1, "  Author • 3 weeks ago • add feature".to_string());

    assert_eq!(e.line_annotations.len(), 2);
    assert_eq!(
        e.line_annotations.get(&0).map(String::as_str),
        Some("  Author • 2 days ago • fix bug")
    );
}

#[test]
fn line_annotations_cleared_when_switching_files() {
    let mut e = engine_with("hello\n");
    e.line_annotations.insert(0, "blame text".to_string());
    assert!(!e.line_annotations.is_empty());

    // open_file_in_tab() clears line_annotations at the top.
    // :e returns EngineAction::OpenFile (processed by UI), so we call
    // open_file_in_tab directly here to test the clearing behavior.
    let path = std::env::temp_dir().join("vimcode_test_ann_clear.txt");
    std::fs::write(&path, "new content\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    assert!(
        e.line_annotations.is_empty(),
        "line_annotations should be cleared after opening a different file"
    );
}

#[test]
fn prompted_extensions_tracks_shown_hints() {
    let mut e = engine_with("");
    assert!(e.prompted_extensions.is_empty());

    // Simulate the engine recording that it already prompted for csharp
    e.prompted_extensions.insert("csharp".to_string());

    assert!(e.prompted_extensions.contains("csharp"));
    assert!(!e.prompted_extensions.contains("python"));
}

// ── Extension manifest lookup via new APIs ────────────────────────────────────

#[test]
fn find_manifest_by_file_ext() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".cs").expect(".cs should map to csharp");
    assert_eq!(m.name, "csharp");
    assert!(m.language_ids.contains(&"csharp".to_string()));
}

#[test]
fn find_manifest_by_language_id() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();
    let m = find_manifest_for_language_id(&manifests, "python")
        .expect("python language id should resolve");
    assert_eq!(m.name, "python");
}

#[test]
fn find_manifest_by_name() {
    use vimcode_core::core::extensions::find_manifest_by_name;
    let manifests = test_manifests();
    assert!(find_manifest_by_name(&manifests, "rust").is_some());
    assert!(find_manifest_by_name(&manifests, "go").is_some());
    assert!(find_manifest_by_name(&manifests, "java").is_some());
    assert!(find_manifest_by_name(&manifests, "nonexistent-xyz").is_none());
}

// ── :ExtRemove ────────────────────────────────────────────────────────────────

#[test]
fn ext_remove_unmarks_installed_extension() {
    let mut e = engine_with("");
    // Mark an extension as installed first
    e.extension_state.mark_installed("python");
    assert!(e.extension_state.is_installed("python"));

    // Remove it via command — now shows a confirmation dialog
    exec(&mut e, "ExtRemove python");
    assert!(e.dialog.is_some(), "dialog should be open");
    // Confirm removal by pressing 'r' (Remove hotkey)
    e.handle_key("", Some('r'), false);
    assert!(
        !e.extension_state.is_installed("python"),
        "python should no longer be installed after :ExtRemove"
    );
}

#[test]
fn ext_remove_unknown_extension_shows_message() {
    let mut e = engine_with("");
    exec(&mut e, "ExtRemove nonexistent-xyz");
    // Dialog opens even for unknown extensions
    assert!(e.dialog.is_some(), "dialog should be open");
    // Confirm removal
    e.handle_key("", Some('r'), false);
    let msg = e.message.to_lowercase();
    // Should show some kind of error or removal message
    assert!(
        msg.contains("nonexistent") || msg.contains("removed") || msg.contains("not"),
        "ExtRemove of unknown extension should give feedback: {msg}"
    );
}

#[test]
fn ext_remove_does_not_affect_other_extensions() {
    let mut e = engine_with("");
    e.extension_state.mark_installed("python");
    e.extension_state.mark_installed("rust");

    exec(&mut e, "ExtRemove python");
    // Confirm dialog
    e.handle_key("", Some('r'), false);

    assert!(
        !e.extension_state.is_installed("python"),
        "python should be removed"
    );
    assert!(
        e.extension_state.is_installed("rust"),
        "rust should remain installed"
    );
}

// ── ext_available_manifests / registry ────────────────────────────────────────

#[test]
fn ext_available_manifests_includes_registry() {
    let e = engine_with_registry("");
    let manifests = e.ext_available_manifests();
    let names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"csharp"), "manifests should include csharp");
    assert!(names.contains(&"python"), "manifests should include python");
    assert!(names.contains(&"rust"), "manifests should include rust");
}

#[test]
fn ext_available_manifests_registry_overrides() {
    use vimcode_core::core::extensions::ExtensionManifest;
    let mut e = engine_with("");
    // Inject a registry with a custom entry
    let mut override_manifest = ExtensionManifest::default();
    override_manifest.name = "rust".to_string();
    override_manifest.display_name = "Rust (Registry Override)".to_string();
    override_manifest.description = "Registry version of rust".to_string();

    e.ext_registry = Some(vec![override_manifest]);
    let manifests = e.ext_available_manifests();
    let rust = manifests
        .iter()
        .find(|m| m.name == "rust")
        .expect("rust should be in manifests");
    assert_eq!(
        rust.display_name, "Rust (Registry Override)",
        "registry entry should be present"
    );
}

#[test]
fn ext_available_manifests_adds_new_registry_entries() {
    use vimcode_core::core::extensions::ExtensionManifest;
    let mut e = engine_with_registry("");
    // Add a custom entry to the existing registry
    let mut reg = e.ext_registry.take().unwrap();
    let mut new_manifest = ExtensionManifest::default();
    new_manifest.name = "custom-extension".to_string();
    new_manifest.display_name = "Custom Extension".to_string();
    reg.push(new_manifest);
    e.ext_registry = Some(reg);

    let manifests = e.ext_available_manifests();
    assert!(
        manifests.iter().any(|m| m.name == "custom-extension"),
        "new registry entry should appear in manifests"
    );
    // Existing entries should still be there
    assert!(
        manifests.iter().any(|m| m.name == "csharp"),
        "csharp should still appear"
    );
}

// ── ext_sidebar_* state ───────────────────────────────────────────────────────

#[test]
fn ext_sidebar_default_state() {
    let e = engine_with("");
    assert!(!e.ext_sidebar_has_focus);
    assert_eq!(e.ext_sidebar_selected, 0);
    assert!(e.ext_sidebar_query.is_empty());
    assert_eq!(e.ext_sidebar_sections_expanded, [true, true]);
    assert!(!e.ext_sidebar_input_active);
    assert!(!e.ext_registry_fetching);
    assert!(e.ext_registry.is_none());
}

#[test]
fn ext_sidebar_key_j_moves_selection_down() {
    let mut e = engine_with_registry("");
    ext_sidebar_setup(&mut e, 1, 0);
    e.dispatch_ext_sidebar_key_unified("j", None);
    let (_, idx) = e.ext_selected_from_sidebar_system();
    assert!(idx > 0, "j should move selection down: selected={}", idx);
}

#[test]
fn ext_sidebar_key_k_moves_selection_up() {
    let mut e = engine_with_registry("");
    ext_sidebar_setup(&mut e, 1, 2);
    e.dispatch_ext_sidebar_key_unified("k", None);
    let (_, idx) = e.ext_selected_from_sidebar_system();
    assert!(idx < 2, "k should move selection up: selected={}", idx);
}

#[test]
fn ext_sidebar_key_escape_unfocuses() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.dispatch_ext_sidebar_key_unified("Escape", None);
    assert!(!e.ext_sidebar_has_focus, "Escape should unfocus sidebar");
}

#[test]
fn ext_sidebar_key_slash_activates_search_input() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.dispatch_ext_sidebar_key_unified("/", None);
    assert!(
        e.ext_sidebar_input_active,
        "/ should activate search input mode"
    );
}

#[test]
fn ext_sidebar_search_filters_manifests() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_query = "rust".to_string();
    let available = e.ext_available_manifests();
    // Filter by query manually
    let q = "rust";
    let filtered: Vec<_> = available
        .iter()
        .filter(|m| m.name.to_lowercase().contains(q) || m.display_name.to_lowercase().contains(q))
        .collect();
    assert!(
        !filtered.is_empty(),
        "searching for 'rust' should find at least one extension"
    );
}

// ── :LspInstall redirect ───────────────────────────────────────────────────────

#[test]
fn lsp_install_redirects_to_ext_install() {
    let mut e = engine_with("");
    exec(&mut e, "LspInstall rust");
    let msg = &e.message;
    assert!(
        msg.contains("ExtInstall") || msg.contains("rust"),
        ":LspInstall should redirect to :ExtInstall: {msg}"
    );
    assert!(
        !msg.contains("No LSP"),
        ":LspInstall should not emit 'No LSP' message: {msg}"
    );
}

// ── :ExtRefresh ───────────────────────────────────────────────────────────────

#[test]
fn ext_refresh_sets_fetching_flag() {
    let mut e = engine_with("");
    assert!(!e.ext_registry_fetching);
    // ext_refresh() should spawn a background thread and set fetching=true
    e.ext_refresh();
    assert!(
        e.ext_registry_fetching,
        "ext_refresh should set ext_registry_fetching=true"
    );
    // Clean up: drop the receiver so the thread doesn't block
    e.ext_registry_rx = None;
    e.ext_registry_fetching = false;
}

// ── find_manifest_for_file_ext — all primary extensions ─────────────────────

#[test]
fn find_for_file_ext_rs_maps_to_rust() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".rs").expect(".rs should map to rust");
    assert_eq!(m.name, "rust");
}

#[test]
fn find_for_file_ext_py_maps_to_python() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".py").expect(".py should map to python");
    assert_eq!(m.name, "python");
}

#[test]
fn find_for_file_ext_go_maps_to_go() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".go").expect(".go should map to go");
    assert_eq!(m.name, "go");
}

#[test]
fn find_for_file_ext_js_maps_to_javascript() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".js").expect(".js should map to javascript");
    assert_eq!(m.name, "javascript");
}

#[test]
fn find_for_file_ext_ts_maps_to_javascript() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    // TypeScript is in the javascript extension
    let m = find_manifest_for_file_ext(&manifests, ".ts")
        .expect(".ts should map to javascript extension");
    assert_eq!(m.name, "javascript");
}

#[test]
fn find_for_file_ext_cpp_maps_to_cpp() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".cpp").expect(".cpp should map to cpp");
    assert_eq!(m.name, "cpp");
}

#[test]
fn find_for_file_ext_c_maps_to_cpp() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".c").expect(".c should map to cpp extension");
    assert_eq!(m.name, "cpp");
}

#[test]
fn find_for_file_ext_java_maps_to_java() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".java").expect(".java should map to java");
    assert_eq!(m.name, "java");
}

#[test]
fn find_for_file_ext_php_maps_to_php() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".php").expect(".php should map to php");
    assert_eq!(m.name, "php");
}

#[test]
fn find_for_file_ext_rb_maps_to_ruby() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".rb").expect(".rb should map to ruby");
    assert_eq!(m.name, "ruby");
}

#[test]
fn find_for_file_ext_sh_maps_to_bash() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    let m = find_manifest_for_file_ext(&manifests, ".sh").expect(".sh should map to bash");
    assert_eq!(m.name, "bash");
}

#[test]
fn find_for_file_ext_unknown_returns_none() {
    use vimcode_core::core::extensions::find_manifest_for_file_ext;
    let manifests = test_manifests();
    assert!(
        find_manifest_for_file_ext(&manifests, ".xyz123").is_none(),
        ".xyz123 should not map to any extension"
    );
}

// ── find_manifest_for_language_id — gaps not covered by earlier tests ─────────

#[test]
fn find_for_language_id_typescript_maps_to_javascript() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();
    let m = find_manifest_for_language_id(&manifests, "typescript")
        .expect("typescript lang id should resolve to javascript");
    assert_eq!(m.name, "javascript");
}

#[test]
fn find_for_language_id_c_maps_to_cpp() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();
    let m =
        find_manifest_for_language_id(&manifests, "c").expect("c lang id should resolve to cpp");
    assert_eq!(m.name, "cpp");
}

#[test]
fn find_for_language_id_shellscript_maps_to_bash() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();
    let m = find_manifest_for_language_id(&manifests, "shellscript")
        .expect("shellscript lang id should resolve to bash");
    assert_eq!(m.name, "bash");
}

#[test]
fn find_for_language_id_unknown_returns_none() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();
    assert!(find_manifest_for_language_id(&manifests, "cobol2024").is_none());
}

// ── :ExtInstall command behaviour ─────────────────────────────────────────────

#[test]
fn ext_install_known_extension_marks_installed() {
    let mut e = engine_with_registry("");
    assert!(!e.extension_state.is_installed("git-insights"));
    // git-insights has no LSP/DAP install command — safe to call in tests
    exec(&mut e, "ExtInstall git-insights");
    assert!(
        e.extension_state.is_installed("git-insights"),
        "git-insights should be marked installed after :ExtInstall"
    );
}

#[test]
fn ext_install_shows_installing_message() {
    let mut e = engine_with_registry("");
    exec(&mut e, "ExtInstall git-insights");
    assert!(
        e.message.to_lowercase().contains("installing")
            || e.message.to_lowercase().contains("install"),
        "message after :ExtInstall should mention installing: {}",
        e.message
    );
}

#[test]
fn ext_install_unknown_extension_shows_error() {
    let mut e = engine_with_registry("");
    exec(&mut e, "ExtInstall nonexistent-xyz-extension");
    let msg = e.message.to_lowercase();
    assert!(
        msg.contains("unknown") || msg.contains("not found") || msg.contains("nonexistent"),
        "message for unknown extension should be an error: {}",
        e.message
    );
    assert!(
        !e.extension_state.is_installed("nonexistent-xyz-extension"),
        "unknown extension should not be marked installed"
    );
}

// ── Terminal-based install ─────────────────────────────────────────────────────
//
// #1703 / #1712: these two tests used to drive the `ruby` fixture on the
// strength of "`ruby-lsp` is unlikely to be on PATH in CI", which made them
// pass or fail according to what the host machine happened to have installed.
// *Both* issues hit it independently — #1712 on a macOS box with
// `brew install ruby-lsp`, #1703 in its own Test stage — which is how two
// separate fixes for one flake came to be written in parallel. There are
// *two* independent host dependencies, and the fixtures below remove both:
//
//  1. `ext_install_from_registry` only emits a terminal install command when
//     the manifest's LSP binary cannot be resolved — and resolution
//     (`lsp_manager::resolve_command`) spans far more than `PATH`: the
//     vimcode-managed tools dir (#1345), Mason's `bin`, `~/.dotnet/tools`,
//     `~/.cargo/bin`, `~/.local/bin`, `~/go/bin`, `~/.npm-global/bin` and
//     the Homebrew prefixes (#917/#1344). Any one of those holding a
//     `ruby-lsp` turned the assertion below into "got: None". This is the
//     dependency #1712 diagnosed, and no amount of env scrubbing hides it.
//  2. `ext_available_manifests()` merges
//     `~/.config/vimcode/extensions/*/manifest.toml` *over* the registry, so
//     a developer who has really installed `ruby` gets the published
//     manifest instead of the fixture — and that one may acquire its LSP
//     natively (#1345), which deliberately sets no terminal command at all.
//
// (2) is also why #1712's narrower fix — reach into `e.ext_registry` and
// rename the `ruby` entry's `lsp.binary` to a sentinel — could not be the
// whole answer, and why the two fixes were reconciled in favour of this one
// rather than merged side by side: the install path resolves its manifest
// through `ext_available_manifests()`, not through `ext_registry` directly,
// so a local `ruby` manifest directory shadows the mutation and the test
// goes red again on exactly the kind of box that reported the flake. A
// synthetic registry entry closes both holes at once, and `..Default()` on
// `LspConfig` below carries #1712's `fallback_binaries.clear()` for free —
// an empty fallback list is the default, so there is no second binary for
// the resolver to find either.
//
// `UNRESOLVABLE_LSP_BINARY` / `TERMINAL_INSTALL_EXT` are names that cannot
// exist in any probed tool directory or extension directory on any host, so
// the "LSP binary missing → terminal install" branch is the only branch these
// tests can take. Note `LspConfig::install` (not `install_linux`) — it is the
// fallback for every platform, so this stays host-OS-neutral.

/// An LSP binary name no machine can resolve. See the block comment above.
const UNRESOLVABLE_LSP_BINARY: &str = "vimcode-test-1703-unresolvable-lsp";

/// An extension name no on-disk local manifest can shadow. See above.
const TERMINAL_INSTALL_EXT: &str = "vimcode-test-1703-terminal-install";

/// Engine whose registry holds exactly one extension whose only install
/// route is a terminal command: an unresolvable LSP binary, a plain
/// `install` string, no `[lsp.acquire]`, no DAP, no scripts.
fn engine_with_terminal_install_ext() -> vimcode_core::Engine {
    use vimcode_core::core::extensions::*;
    let mut e = engine_with("");
    e.ext_registry = Some(vec![ExtensionManifest {
        name: TERMINAL_INSTALL_EXT.to_string(),
        display_name: "Terminal-install Fixture (#1703)".to_string(),
        file_extensions: vec![".vimcode1703".to_string()],
        language_ids: vec!["vimcode1703".to_string()],
        lsp: LspConfig {
            binary: UNRESOLVABLE_LSP_BINARY.to_string(),
            install: format!("gem install {UNRESOLVABLE_LSP_BINARY}"),
            ..Default::default()
        },
        ..Default::default()
    }]);
    e
}

#[test]
fn ext_install_sets_pending_terminal_command_for_lsp() {
    let mut e = engine_with_terminal_install_ext();
    let action = exec(&mut e, &format!("ExtInstall {TERMINAL_INSTALL_EXT}"));
    assert!(
        e.extension_state.is_installed(TERMINAL_INSTALL_EXT),
        "{TERMINAL_INSTALL_EXT} should be marked installed"
    );
    // The action should be RunInTerminal (carries the install command).
    let cmd = match &action {
        vimcode_core::EngineAction::RunInTerminal(cmd) => cmd.clone(),
        other => panic!("should return RunInTerminal action, got: {other:?}"),
    };
    assert!(
        cmd.contains(UNRESOLVABLE_LSP_BINARY),
        "terminal command should carry the install string: {cmd}"
    );
    // Clean up
    e.extension_state.installed.clear();
}

#[test]
fn ext_install_no_terminal_command_when_binary_exists() {
    let mut e = engine_with_registry("");
    // git-insights has no LSP/DAP install command, so no terminal command.
    let action = exec(&mut e, "ExtInstall git-insights");
    assert!(
        e.extension_state.is_installed("git-insights"),
        "git-insights should be marked installed"
    );
    assert!(
        e.pending_terminal_command.is_none(),
        "no terminal command for extension without install"
    );
    assert_eq!(
        action,
        vimcode_core::EngineAction::None,
        "should return None action"
    );
    // Clean up
    e.extension_state.installed.clear();
}

#[test]
fn ext_install_sets_install_context_for_lsp() {
    // The `:ExtInstall` path hands the command back as an `EngineAction`, which
    // consumes `pending_terminal_command`; the sidebar path cannot return an
    // action, so it has to leave the command *on the engine* for the backend to
    // pick up. That is what this test covers — drive the sidebar's `i` key.
    let mut e = engine_with_terminal_install_ext();
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, true];
    let available = e.ext_available_items();
    let ext_idx = available
        .iter()
        .position(|m| m.name == TERMINAL_INSTALL_EXT)
        .unwrap_or_else(|| panic!("{TERMINAL_INSTALL_EXT} should be in available"));
    ext_sidebar_setup(&mut e, 1, ext_idx);
    e.dispatch_ext_sidebar_key_unified("i", None);

    // pending_terminal_command should be set (sidebar can't return EngineAction)
    assert!(
        e.pending_terminal_command.is_some(),
        "sidebar install should set pending_terminal_command"
    );
    let cmd = e.pending_terminal_command.as_ref().unwrap();
    assert!(
        cmd.contains(UNRESOLVABLE_LSP_BINARY),
        "command should mention {UNRESOLVABLE_LSP_BINARY}: {cmd}"
    );
    // Clean up
    e.extension_state.installed.clear();
    e.pending_terminal_command = None;
    e.pending_install_context = None;
}

// ── Auto-offer on file open ───────────────────────────────────────────────────
//
// #1397 moved the "recommended extension isn't installed" offer off the
// status line (where any later `Engine::message` write silently ate it
// before the user looked) and onto an actionable, sticky toast. #1577
// rebuilt it on quadraui#1185's multi-action toast: "Install" and "Don't
// ask again" are now separate buttons instead of one action button plus
// keyboard shortcuts printed as body text. These tests therefore read the
// offer back through the **render layer** (`render::build_toast_stack` —
// the exact struct both the TUI and GTK backends hand to
// `draw_toast_overlay`), not through `Engine::toasts`: a toast sitting in
// engine state that never makes it into a paintable stack must still fail
// here. Full painted-pixel coverage of the offer — title, both buttons,
// click, × dismiss, keyboard focus — lives in the `TuiDriver` tests in
// `src/tui_main/app_on_tui_tests.rs` and the `GtkDriver` tests in
// `src/gtk/testing.rs`.

/// The install-offer toast currently in the *rendered* toast stack, as
/// `(title, body, action_button_labels)`. `None` when nothing is offering
/// an install — either no toast at all, or only plain (action-less)
/// toasts such as the LSP-failure notice.
fn install_offer_toast(e: &vimcode_core::Engine) -> Option<(String, String, Vec<String>)> {
    let stack = vimcode_core::render::build_toast_stack(e)?;
    stack.toasts.into_iter().find_map(|t| {
        if t.actions.is_empty() {
            None
        } else {
            let labels = t.actions.into_iter().map(|a| a.label).collect();
            Some((t.title, t.body, labels))
        }
    })
}

#[test]
fn auto_offer_toast_shown_for_uninstalled_extension_on_file_open() {
    let mut e = engine_with_registry("");
    assert!(!e.extension_state.is_installed("csharp"));
    assert!(!e.extension_state.is_dismissed("csharp"));

    let path = std::env::temp_dir().join("vimcode_smoke_hint_01.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    let (title, body, labels) = install_offer_toast(&e).unwrap_or_else(|| {
        panic!(
            "expected an install-offer toast for uninstalled csharp; \
             message was {:?}",
            e.message
        )
    });
    assert!(
        title.contains("C# Language Support"),
        "offer title should name the extension: {title}"
    );
    assert_eq!(
        labels,
        vec!["Install".to_string(), "Don't ask again".to_string()],
        "offer needs both an Install and a Don't ask again action button"
    );
    assert!(
        body.contains("C# Language Support"),
        "offer body should say what installing adds: {body}"
    );
}

#[test]
fn auto_offer_toast_not_shown_when_extension_dismissed() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_dismissed("csharp");

    let path = std::env::temp_dir().join("vimcode_smoke_hint_02.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        install_offer_toast(&e),
        None,
        "offer should not appear when csharp is dismissed"
    );
}

#[test]
fn auto_offer_toast_not_shown_when_extension_installed() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");

    let path = std::env::temp_dir().join("vimcode_smoke_hint_03.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        install_offer_toast(&e),
        None,
        "offer should not appear when csharp is installed"
    );
}

#[test]
fn auto_offer_toast_not_shown_twice_for_same_extension() {
    let mut e = engine_with_registry("");

    let path1 = std::env::temp_dir().join("vimcode_smoke_hint_04a.cs");
    let path2 = std::env::temp_dir().join("vimcode_smoke_hint_04b.cs");
    std::fs::write(&path1, "// a\n").ok();
    std::fs::write(&path2, "// b\n").ok();

    e.open_file_in_tab(&path1);
    let first = install_offer_toast(&e);
    // Dismissing (×) the first offer is what a user who ignored it does;
    // the second open must not bring it back.
    e.toasts.clear();
    e.open_file_in_tab(&path2);
    let second = install_offer_toast(&e);

    let _ = std::fs::remove_file(&path1);
    let _ = std::fs::remove_file(&path2);

    // First open should have triggered the offer
    assert!(
        first.is_some(),
        "first open should show the install offer; message was {:?}",
        e.message
    );
    // Second open of same language must NOT re-offer
    assert_eq!(second, None, "second open should not re-prompt for csharp");
}

/// #1397: the offer must survive `prune_toasts` — a 5s auto-expiry would
/// silently revert an unanswered offer to "never asked", with nothing on
/// screen to show the user was ever asked.
#[test]
fn auto_offer_toast_is_sticky_and_survives_pruning() {
    let mut e = engine_with_registry("");

    let path = std::env::temp_dir().join("vimcode_smoke_hint_05.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    let before = install_offer_toast(&e);
    assert!(before.is_some(), "precondition: offer must be showing");

    // Backdate the toast well past TOAST_LIFETIME, then prune.
    for t in e.toasts.iter_mut() {
        t.created_at = std::time::Instant::now() - std::time::Duration::from_secs(600);
    }
    e.prune_toasts();

    assert_eq!(
        install_offer_toast(&e),
        before,
        "the install offer is sticky — pruning must not drop it"
    );
}

// ── Sidebar navigation — clamping ─────────────────────────────────────────────

#[test]
fn ext_sidebar_j_clamps_at_last_item() {
    let mut e = engine_with_registry("");
    let total = e.ext_available_manifests().len();
    ext_sidebar_setup(&mut e, 1, total.saturating_sub(1));
    e.dispatch_ext_sidebar_key_unified("j", None);
    let (_, idx) = e.ext_selected_from_sidebar_system();
    assert!(
        idx < total,
        "j should not go past the last item: selected={idx}, total={total}",
    );
}

#[test]
fn ext_sidebar_k_clamps_at_zero() {
    let mut e = engine_with_registry("");
    ext_sidebar_setup(&mut e, 1, 0);
    e.dispatch_ext_sidebar_key_unified("k", None);
    let (_, idx) = e.ext_selected_from_sidebar_system();
    assert_eq!(idx, 0, "k at position 0 should stay at 0");
}

// ── Sidebar Tab — section toggling ────────────────────────────────────────────

#[test]
fn ext_sidebar_tab_toggles_installed_section() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");
    ext_sidebar_setup(&mut e, 0, 0);
    e.dispatch_ext_sidebar_key_unified("Tab", None);
    let sidebar = e.ext_sidebar_system.borrow();
    assert_eq!(
        sidebar.active_section(),
        Some(1),
        "Tab should cycle from installed (0) to available (1)"
    );
}

#[test]
fn ext_sidebar_tab_toggles_available_section_when_no_installed() {
    let mut e = engine_with_registry("");
    ext_sidebar_setup(&mut e, 1, 0);
    e.dispatch_ext_sidebar_key_unified("Tab", None);
    let sidebar = e.ext_sidebar_system.borrow();
    assert_eq!(
        sidebar.active_section(),
        Some(0),
        "Tab should cycle from available (1) back to installed (0)"
    );
}

// ── Sidebar d — remove installed extension ────────────────────────────────────

#[test]
fn ext_sidebar_d_removes_installed_extension() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_selected = 0; // first (and only) installed item

    e.dispatch_ext_sidebar_key_unified("d", None);
    // Dialog should be open — confirm removal.
    // Navigate Right (past Cancel) then Enter. In 2-button dialog this is
    // "Remove"; in 3-button (tools on PATH) this is "Keep Tools".
    // Both remove the extension without deleting system tool binaries.
    assert!(e.dialog.is_some(), "removal dialog should be open");
    e.handle_key("Right", None, false);
    e.handle_key("Return", None, false);

    assert!(
        !e.extension_state.is_installed("csharp"),
        "csharp should be removed after d in sidebar"
    );
    assert!(
        e.message.contains("removed") || e.message.contains("csharp"),
        "message should confirm removal: {}",
        e.message
    );
}

#[test]
fn ext_sidebar_d_on_available_item_is_noop() {
    let mut e = engine_with_registry("");
    // No extensions installed — selected is in the available section
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_selected = 0;

    let msg_before = e.message.clone();
    e.dispatch_ext_sidebar_key_unified("d", None);
    // Should not crash; message may or may not change (no-op on available items)
    // The important thing is no extension gets spuriously marked removed
    let total_installed = e
        .ext_available_manifests()
        .iter()
        .filter(|m| e.extension_state.is_installed(&m.name))
        .count();
    assert_eq!(
        total_installed, 0,
        "d on available item should not remove anything; msg_before={msg_before}"
    );
}

// ── Sidebar Return ─────────────────────────────────────────────────────────────

#[test]
fn ext_sidebar_return_on_installed_opens_readme() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, false];
    e.ext_sidebar_selected = 0;

    let _tabs_before = e.active_group().tabs.len();
    e.dispatch_ext_sidebar_key_unified("Return", None);

    // Should not crash and should not trigger a re-install
    assert!(
        !e.message.to_lowercase().contains("installing"),
        "Return on installed item should not trigger re-install: {}",
        e.message
    );
    // May or may not open a tab (README may not be available on disk in tests)
    // Just verify it doesn't panic
    let _ = e.active_group().tabs.len();
}

#[test]
fn ext_sidebar_return_on_available_opens_readme_without_install() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, true];
    // Select first available item (nothing installed)
    e.ext_sidebar_selected = 0;

    e.dispatch_ext_sidebar_key_unified("Return", None);

    // Should not install
    assert!(
        !e.extension_state.is_installed("bash"),
        "Return on available extension should NOT install: got {:?}",
        e.extension_state.installed
    );
}

#[test]
fn ext_sidebar_i_on_already_installed_shows_message() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, false];
    e.ext_sidebar_selected = 0;

    e.dispatch_ext_sidebar_key_unified("i", None);

    assert!(
        e.message.contains("already installed"),
        "i on installed extension should say already installed: {}",
        e.message
    );
}

// ── Sidebar search input mode ──────────────────────────────────────────────────

#[test]
fn ext_sidebar_search_input_accumulates_typed_chars() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_input_active = true;

    e.dispatch_ext_sidebar_key_unified("r", Some('r'));
    e.dispatch_ext_sidebar_key_unified("u", Some('u'));
    e.dispatch_ext_sidebar_key_unified("s", Some('s'));
    e.dispatch_ext_sidebar_key_unified("t", Some('t'));

    assert_eq!(
        e.ext_sidebar_query, "rust",
        "typed characters should accumulate in sidebar query"
    );
}

#[test]
fn ext_sidebar_search_escape_deactivates_and_preserves_query() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_input_active = true;
    e.ext_sidebar_query = "rust".to_string();

    e.dispatch_ext_sidebar_key_unified("Escape", None);

    assert!(
        !e.ext_sidebar_input_active,
        "Escape should deactivate search input"
    );
    assert_eq!(
        e.ext_sidebar_query, "rust",
        "Escape should preserve the query string"
    );
}

#[test]
fn ext_sidebar_search_backspace_removes_last_char() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_input_active = true;
    e.ext_sidebar_query = "rust".to_string();

    e.dispatch_ext_sidebar_key_unified("BackSpace", None);

    assert_eq!(
        e.ext_sidebar_query, "rus",
        "BackSpace should remove the last char from the query"
    );
}

#[test]
fn ext_sidebar_search_appends_to_query_on_input() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_input_active = true;
    e.ext_sidebar_query = "rus".to_string();

    e.dispatch_ext_sidebar_key_unified("t", Some('t'));

    assert_eq!(
        e.ext_sidebar_query, "rust",
        "typing in search should append to query"
    );
}

// ── Settings: extension_registries ────────────────────────────────────────────

#[test]
fn extension_registries_has_default_url() {
    let s = vimcode_core::Settings::default();
    assert!(
        !s.extension_registries.is_empty(),
        "extension_registries should have at least one default URL"
    );
    assert!(
        s.extension_registries[0].starts_with("http"),
        "first registry URL should be an http(s) URL: {}",
        s.extension_registries[0]
    );
}

#[test]
fn extension_registries_set_via_comma_separated() {
    let mut s = vimcode_core::Settings::default();
    s.set_value_str(
        "extension_registries",
        "https://a.example.com/registry.json, https://b.example.com/registry.json",
    )
    .unwrap();
    assert_eq!(s.extension_registries.len(), 2);
    assert_eq!(
        s.extension_registries[0],
        "https://a.example.com/registry.json"
    );
    assert_eq!(
        s.extension_registries[1],
        "https://b.example.com/registry.json"
    );
}

#[test]
fn extension_registries_get_value_str() {
    let s = vimcode_core::Settings::default();
    let val = s.get_value_str("extension_registries");
    assert!(
        val.contains("https://"),
        "get_value_str should return URLs: {}",
        val
    );
}

#[test]
fn extension_registries_query_via_set() {
    let mut s = vimcode_core::Settings::default();
    let result = s.parse_set_option("extension_registries?").unwrap();
    assert!(
        result.starts_with("extension_registries="),
        "query should start with 'extension_registries=': {}",
        result
    );
}

// ── ext_remove edge cases ─────────────────────────────────────────────────────

#[test]
fn ext_remove_on_not_installed_extension_shows_message() {
    let mut e = engine_with("");
    assert!(!e.extension_state.is_installed("ruby"));

    exec(&mut e, "ExtRemove ruby");
    // Confirm dialog
    assert!(e.dialog.is_some(), "dialog should be open");
    e.handle_key("", Some('r'), false);

    // ext_remove always shows a message even when the extension wasn't installed
    let msg = e.message.to_lowercase();
    assert!(
        msg.contains("ruby") || msg.contains("removed") || msg.contains("not"),
        "ext_remove should give feedback even when not installed: {}",
        e.message
    );
}

// ── Manifest-driven LSP/DAP lookup ────────────────────────────────────────────

#[test]
fn manifest_lsp_fallback_binaries_parsed() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();
    let m = find_manifest_for_language_id(&manifests, "python").expect("python manifest");
    assert!(
        !m.lsp.fallback_binaries.is_empty(),
        "python should have lsp.fallback_binaries"
    );
    assert!(
        m.lsp
            .fallback_binaries
            .contains(&"basedpyright-langserver".to_string()),
        "fallbacks should contain basedpyright-langserver: {:?}",
        m.lsp.fallback_binaries
    );
    assert!(
        m.lsp.fallback_binaries.contains(&"pylsp".to_string()),
        "fallbacks should contain pylsp"
    );
    assert!(
        m.lsp
            .fallback_binaries
            .contains(&"jedi-language-server".to_string()),
        "fallbacks should contain jedi-language-server"
    );
}

#[test]
fn manifest_dap_config_fields_parsed() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();

    // Go: has full DAP config with install command
    let m = find_manifest_for_language_id(&manifests, "go").expect("go manifest");
    assert_eq!(m.dap.binary, "dlv", "go dap binary should be dlv");
    assert_eq!(m.dap.transport, "stdio", "go dap transport should be stdio");
    assert_eq!(m.dap.args, vec!["dap"], "go dap args should be [dap]");
    assert!(
        !m.dap.install.is_empty(),
        "go dap should have an install command"
    );
    assert!(
        m.dap.install.contains("go install"),
        "go dap install should use `go install`: {}",
        m.dap.install
    );

    // Rust: TCP transport for codelldb
    let m = find_manifest_for_language_id(&manifests, "rust").expect("rust manifest");
    assert_eq!(m.dap.binary, "codelldb");
    assert_eq!(m.dap.transport, "tcp");
    assert!(m.dap.args.contains(&"--port".to_string()));
}

#[test]
fn manifest_workspace_markers_parsed_for_multiple_languages() {
    use vimcode_core::core::extensions::find_manifest_for_language_id;
    let manifests = test_manifests();

    let m = find_manifest_for_language_id(&manifests, "rust").expect("rust manifest");
    assert!(
        m.workspace_markers.contains(&"Cargo.toml".to_string()),
        "rust should have Cargo.toml as workspace marker"
    );

    let m = find_manifest_for_language_id(&manifests, "go").expect("go manifest");
    assert!(
        m.workspace_markers.contains(&"go.mod".to_string()),
        "go should have go.mod as workspace marker"
    );

    let m = find_manifest_for_language_id(&manifests, "python").expect("python manifest");
    assert!(
        m.workspace_markers.contains(&"pyproject.toml".to_string()),
        "python should have pyproject.toml as workspace marker"
    );

    let m = find_manifest_for_language_id(&manifests, "javascript").expect("javascript manifest");
    assert!(
        m.workspace_markers.contains(&"package.json".to_string()),
        "javascript should have package.json as workspace marker"
    );
}

#[test]
fn find_workspace_root_uses_manifest_markers() {
    use std::fs;
    use vimcode_core::core::dap_manager::find_workspace_root;

    // Create a temp dir with a go.mod (a marker from the Go manifest)
    let tmp = std::env::temp_dir().join("vimcode_test_wsroot_go");
    let sub = tmp.join("src").join("pkg");
    fs::create_dir_all(&sub).ok();
    fs::write(tmp.join("go.mod"), "module example.com/mymod\n").ok();

    // Start from a deep subdirectory — should walk up to tmp.
    let manifests = test_manifests();
    let root = find_workspace_root(&sub, &manifests);
    assert_eq!(
        root, tmp,
        "should find go.mod in parent dir via manifest marker"
    );

    // Cleanup
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn find_workspace_root_uses_gemfile_marker() {
    use std::fs;
    use vimcode_core::core::dap_manager::find_workspace_root;

    let tmp = std::env::temp_dir().join("vimcode_test_wsroot_ruby");
    let sub = tmp.join("lib");
    fs::create_dir_all(&sub).ok();
    fs::write(tmp.join("Gemfile"), "source 'https://rubygems.org'\n").ok();

    let manifests = test_manifests();
    let root = find_workspace_root(&sub, &manifests);
    assert_eq!(
        root, tmp,
        "should find Gemfile in parent dir via manifest marker"
    );

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn dap_install_cmd_for_go_comes_from_manifest() {
    use vimcode_core::core::dap_manager::install_cmd_for_adapter;
    let manifests = test_manifests();
    let cmd = install_cmd_for_adapter("delve", &manifests);
    assert!(cmd.is_some(), "delve should have an install command");
    let cmd = cmd.unwrap();
    assert!(
        cmd.contains("go install") && cmd.contains("dlv"),
        "delve install cmd should come from go manifest: {cmd}"
    );
}

// ── cursor_move hook ───────────────────────────────────────────────────────────

#[test]
fn fire_cursor_move_hook_doesnt_panic_without_plugin_manager() {
    let mut e = engine_with("hello world\n");
    // plugin_manager is None by default in test engines
    assert!(e.plugin_manager.is_none());
    // This must not panic
    e.fire_cursor_move_hook();
}

#[test]
fn handle_key_fires_cursor_move_when_cursor_moves() {
    let mut e = engine_with("hello world\n");
    // No plugin manager — cursor_move is a no-op, but must not panic
    // We move the cursor with 'l' and ensure no crash
    assert!(e.plugin_manager.is_none());
    press(&mut e, 'l'); // move cursor right
                        // If we get here without panicking, the hook fired safely
    assert_eq!(e.cursor().col, 1, "cursor should have moved right");
}

// ── Ext sidebar navigation regression tests ───────────────────────────────────

/// After pressing Enter to install an extension, the selection should move to
/// the newly installed item in the installed section, not stay at the old
/// available-section index (which would point to a different item after install).
#[test]
fn ext_install_via_return_resets_selection_to_installed_item() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_sections_expanded = [true, true];
    let available_before = e
        .ext_available_manifests()
        .into_iter()
        .filter(|m| !e.extension_state.is_installed(&m.name))
        .collect::<Vec<_>>();
    let rust_idx = available_before
        .iter()
        .position(|m| m.name == "rust")
        .expect("rust should be in available list");
    ext_sidebar_setup(&mut e, 1, rust_idx);

    // Install via 'i' key
    e.dispatch_ext_sidebar_key_unified("i", None);

    // Rust should now be installed
    assert!(
        e.extension_state.is_installed("rust"),
        "rust should be marked installed after i"
    );

    // Selection should now be in the installed section, pointing at rust
    let (in_installed, sel) = e.ext_selected_from_sidebar_system();
    assert!(in_installed, "selection should be in installed section");
    let installed = e.ext_installed_items();
    assert!(
        sel < installed.len(),
        "selection {sel} should be within installed section (len {})",
        installed.len()
    );
    assert_eq!(
        installed[sel].name, "rust",
        "selection should point to rust in installed section"
    );

    // d should now work immediately (without extra navigation)
    e.dispatch_ext_sidebar_key_unified("d", None);
    assert!(e.dialog.is_some(), "removal dialog should be open");
    e.handle_key("Right", None, false);
    e.handle_key("Return", None, false);
    assert!(
        !e.extension_state.is_installed("rust"),
        "rust should be removed after pressing d on newly installed item"
    );

    // Clean up
    e.extension_state.installed.clear();
}

/// After deleting the last installed extension when the available section is
/// collapsed, the available section should be expanded so navigation still works.
#[test]
fn ext_delete_last_installed_expands_available_if_collapsed() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("bash");
    e.ext_sidebar_system.borrow_mut().set_collapsed(1, true);
    ext_sidebar_setup(&mut e, 0, 0);

    let installed_before = e.ext_installed_items();
    assert_eq!(
        installed_before.len(),
        1,
        "should have 1 installed item (bash)"
    );

    // Delete bash — confirm dialog (Right past Cancel, then Enter)
    e.dispatch_ext_sidebar_key_unified("d", None);
    assert!(e.dialog.is_some(), "removal dialog should be open");
    e.handle_key("Right", None, false);
    e.handle_key("Return", None, false);

    assert!(
        !e.extension_state.is_installed("bash"),
        "bash should be removed"
    );

    // The available section should now be expanded in the SidebarSystem
    assert!(
        !e.ext_sidebar_system.borrow().is_collapsed(1),
        "available section should be expanded after deleting last installed item"
    );

    // Available items should still exist
    let available_after: Vec<_> = e
        .ext_available_manifests()
        .into_iter()
        .filter(|m| !e.extension_state.is_installed(&m.name))
        .collect();
    assert!(
        !available_after.is_empty(),
        "available items should be visible after expanding section"
    );

    // Clean up
    e.extension_state.installed.clear();
}

// ═══════════════════════════════════════════════════════════════════════════
// Plugin API — Phase 1: Autocmd events, keymap modes, cursor set, settings,
// state queries
// ═══════════════════════════════════════════════════════════════════════════

/// On-disk home for a plugin fixture named `plugin_name`, unique to *this
/// test process*.
///
/// `std::env::temp_dir()` is shared by every process the user runs, so a path
/// keyed only on `plugin_name` is the *same* directory in two concurrent test
/// runs — two coord worktrees on one machine, or a `--test extensions` run
/// alongside a full `cargo test`. Every fixture helper below starts with
/// `remove_dir_all` and then writes the Lua source, so concurrent runs
/// clobber each other three ways, all of which surface as an assertion
/// failure that looks unrelated to the shared directory:
///
/// * `fs::write` into a directory another run just deleted fails outright
///   (`EINVAL`), panicking in the helper itself;
/// * the last writer wins, so a run can load *another* run's Lua — fatal when
///   the source is interpolated per-run, e.g. the `http_*` fixtures embed
///   their own ephemeral `base_url`, so the losers all aim at one run's
///   single-connection fixture server and their callbacks never fire;
/// * `load_plugins_dir` can observe the directory mid-recreate and load zero
///   plugins, after which every `vimcode.command` the test registered is
///   simply absent.
///
/// Appending the pid gives each run its own fixtures. The pid is stable for
/// the life of the process, so helpers that need to re-derive the same path
/// later (a simulated plugin reload) still agree with the original.
///
/// The pid suffix means a fixed name can no longer self-clean on the next
/// run the way the old shared path did, so `/tmp` would otherwise accumulate
/// one `vc_plugin_api_*_<pid>` tree per `cargo test` invocation forever.
/// There's no portable std API to ask "is pid N still alive" to clean up
/// precisely, so instead: on first use per process, sweep away any sibling
/// fixture dirs whose mtime is old enough that they can only be orphans from
/// a past run (a live run's own dirs were just created, so they're exempt).
fn plugin_fixture_dir(plugin_name: &str) -> std::path::PathBuf {
    cleanup_stale_fixture_dirs();
    std::env::temp_dir().join(format!(
        "vc_plugin_api_{plugin_name}_{}",
        std::process::id()
    ))
}

/// See `plugin_fixture_dir` — best-effort removal of stale fixture dirs left
/// behind by past test runs. Runs at most once per process.
fn cleanup_stale_fixture_dirs() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let stale_cutoff = std::time::Duration::from_secs(3600);
        let now = std::time::SystemTime::now();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with("vc_plugin_api_") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if now.duration_since(modified).unwrap_or_default() >= stale_cutoff {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
}

/// Helper: create an engine with a PluginManager loaded from a temp dir.
fn engine_with_plugin(text: &str, plugin_name: &str, lua_code: &str) -> vimcode_core::Engine {
    let dir = plugin_fixture_dir(plugin_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{plugin_name}.lua")), lua_code).unwrap();

    let mut e = engine_with(text);
    match vimcode_core::core::plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &[]);
            e.set_plugin_manager(mgr);
        }
        Err(_) => panic!("failed to create PluginManager"),
    }
    e
}

// ── vimcode.buf.set_cursor ─────────────────────────────────────────────────

#[test]
fn plugin_set_cursor_moves_cursor() {
    let mut e = engine_with_plugin(
        "line one\nline two\nline three\n",
        "set_cursor",
        r#"
        vimcode.command("GoTo", function(_)
            vimcode.buf.set_cursor(2, 3)
        end)
        "#,
    );
    exec(&mut e, "GoTo");
    assert_eq!(e.cursor().line, 1, "cursor line should be 1 (0-indexed)");
    assert_eq!(e.cursor().col, 2, "cursor col should be 2 (0-indexed)");
}

#[test]
fn plugin_set_cursor_clamps_to_bounds() {
    let mut e = engine_with_plugin(
        "short\n",
        "set_cursor_clamp",
        r#"
        vimcode.command("GoFar", function(_)
            vimcode.buf.set_cursor(999, 999)
        end)
        "#,
    );
    exec(&mut e, "GoFar");
    // Should be clamped to last line, last col
    let max_line = e.buffer().len_lines().saturating_sub(1);
    assert!(
        e.cursor().line <= max_line,
        "cursor line {} should be <= {}",
        e.cursor().line,
        max_line
    );
}

// ── vimcode.opt.get / vimcode.opt.set ──────────────────────────────────────

#[test]
fn plugin_opt_get_reads_settings() {
    let mut e = engine_with_plugin(
        "",
        "opt_get",
        r#"
        vimcode.command("GetTabstop", function(_)
            local ts = vimcode.opt.get("tabstop")
            vimcode.message("tabstop=" .. ts)
        end)
        "#,
    );
    let expected_ts = e.settings.tabstop;
    exec(&mut e, "GetTabstop");
    assert_eq!(
        e.message,
        format!("tabstop={expected_ts}"),
        "opt.get should return current tabstop"
    );
}

#[test]
fn plugin_opt_set_modifies_settings() {
    let mut e = engine_with_plugin(
        "",
        "opt_set",
        r#"
        vimcode.command("SetWrap", function(_)
            vimcode.opt.set("wrap", "true")
        end)
        "#,
    );
    assert!(!e.settings.wrap, "wrap should be false initially");
    exec(&mut e, "SetWrap");
    assert!(e.settings.wrap, "wrap should be true after opt.set");
}

// ── vimcode.state.mode ─────────────────────────────────────────────────────

#[test]
fn plugin_state_mode_returns_current_mode() {
    let mut e = engine_with_plugin(
        "hello\n",
        "state_mode",
        r#"
        vimcode.command("ShowMode", function(_)
            local m = vimcode.state.mode()
            vimcode.message("mode=" .. m)
        end)
        "#,
    );
    // In normal mode
    exec(&mut e, "ShowMode");
    assert_eq!(e.message, "mode=Normal");

    // Enter insert mode and check via command (command runs in normal context,
    // but the ctx is built at call time)
    press(&mut e, 'i');
    // Can't easily exec commands in insert mode, so just verify mode changed
    assert_eq!(e.mode, vimcode_core::Mode::Insert);
}

// ── vimcode.state.register ─────────────────────────────────────────────────

#[test]
fn plugin_state_register_reads_register_content() {
    let mut e = engine_with_plugin(
        "hello world\n",
        "state_register",
        r#"
        vimcode.command("ShowReg", function(_)
            local r = vimcode.state.register("a")
            if r then
                vimcode.message("reg_a=" .. r.content)
            else
                vimcode.message("reg_a=nil")
            end
        end)
        "#,
    );
    // Set register a
    e.registers
        .insert('a', ("test content".to_string(), RegType::Charwise));
    exec(&mut e, "ShowReg");
    assert_eq!(e.message, "reg_a=test content");
}

#[test]
fn plugin_state_register_returns_nil_for_empty() {
    let mut e = engine_with_plugin(
        "",
        "state_register_nil",
        r#"
        vimcode.command("ShowReg", function(_)
            local r = vimcode.state.register("z")
            if r then
                vimcode.message("reg_z=" .. r.content)
            else
                vimcode.message("reg_z=nil")
            end
        end)
        "#,
    );
    exec(&mut e, "ShowReg");
    assert_eq!(e.message, "reg_z=nil");
}

// ── vimcode.state.set_register ─────────────────────────────────────────────

#[test]
fn plugin_state_set_register_writes_register() {
    let mut e = engine_with_plugin(
        "",
        "set_register",
        r#"
        vimcode.command("SetReg", function(_)
            vimcode.state.set_register("b", "plugin text", true)
        end)
        "#,
    );
    exec(&mut e, "SetReg");
    let (content, linewise) = e.registers.get(&'b').expect("register b should be set");
    assert_eq!(content, "plugin text");
    assert!(linewise.is_linewise(), "register should be linewise");
}

// ── vimcode.state.filetype ─────────────────────────────────────────────────

#[test]
fn plugin_state_filetype_returns_language() {
    let mut e = engine_with_plugin(
        "",
        "state_filetype",
        r#"
        vimcode.command("ShowFT", function(_)
            local ft = vimcode.state.filetype()
            vimcode.message("ft=" .. ft)
        end)
        "#,
    );
    // Set up a buffer with a known language ID
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some("rust".to_string());
    }
    exec(&mut e, "ShowFT");
    assert_eq!(e.message, "ft=rust");
}

// ── vimcode.state.mark ─────────────────────────────────────────────────────

#[test]
fn plugin_state_mark_reads_buffer_marks() {
    let mut e = engine_with_plugin(
        "line one\nline two\nline three\n",
        "state_mark",
        r#"
        vimcode.command("ShowMark", function(_)
            local m = vimcode.state.mark("a")
            if m then
                vimcode.message("mark_a=" .. m.line .. "," .. m.col)
            else
                vimcode.message("mark_a=nil")
            end
        end)
        "#,
    );
    // Set mark 'a' at line 1, col 4 (0-indexed)
    let buf_id = e.active_buffer_id();
    e.marks
        .entry(buf_id)
        .or_default()
        .insert('a', vimcode_core::Cursor { line: 1, col: 4 });
    exec(&mut e, "ShowMark");
    // Marks are returned 1-indexed
    assert_eq!(e.message, "mark_a=2,5");
}

// ── vimcode.buf.insert_line / delete_line ──────────────────────────────────

#[test]
fn plugin_buf_insert_line_adds_line() {
    let mut e = engine_with_plugin(
        "line one\nline two\n",
        "insert_line",
        r#"
        vimcode.command("InsLine", function(_)
            vimcode.buf.insert_line(2, "inserted")
        end)
        "#,
    );
    exec(&mut e, "InsLine");
    let content = e.buffer().to_string();
    assert!(
        content.contains("inserted"),
        "buffer should contain inserted line: {content}"
    );
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines[0], "line one", "first line unchanged");
    assert_eq!(lines[1], "inserted", "inserted line at position 2");
    assert_eq!(lines[2], "line two", "original line two shifted down");
}

#[test]
fn plugin_buf_delete_line_removes_line() {
    let mut e = engine_with_plugin(
        "line one\nline two\nline three\n",
        "delete_line",
        r#"
        vimcode.command("DelLine", function(_)
            vimcode.buf.delete_line(2)
        end)
        "#,
    );
    exec(&mut e, "DelLine");
    let content = e.buffer().to_string();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "should have 2 lines after deletion");
    assert_eq!(lines[0], "line one");
    assert_eq!(lines[1], "line three");
}

// ── Visual mode keymap fallback ────────────────────────────────────────────

#[test]
fn plugin_visual_mode_keymap_fires() {
    let mut e = engine_with_plugin(
        "hello world\n",
        "visual_keymap",
        r#"
        vimcode.keymap("v", "Q", function()
            vimcode.message("visual Q fired")
        end)
        "#,
    );
    // Enter visual mode
    press(&mut e, 'v');
    assert_eq!(e.mode, vimcode_core::Mode::Visual);
    // Press Q (should trigger plugin keymap)
    press(&mut e, 'Q');
    assert_eq!(
        e.message, "visual Q fired",
        "visual mode keymap should fire"
    );
}

// ── ModeChanged event ──────────────────────────────────────────────────────

#[test]
fn plugin_mode_changed_event_fires_on_insert() {
    let mut e = engine_with_plugin(
        "hello\n",
        "mode_changed",
        r#"
        vimcode.on("ModeChanged", function(arg)
            vimcode.message("mode_changed:" .. arg)
        end)
        "#,
    );
    // Enter insert mode with 'i'
    press(&mut e, 'i');
    assert!(
        e.message.contains("Normal:Insert"),
        "ModeChanged should fire with Normal:Insert, got: {}",
        e.message
    );
}

#[test]
fn plugin_insert_leave_event_fires() {
    let mut e = engine_with_plugin(
        "hello\n",
        "insert_leave",
        r#"
        vimcode.on("InsertLeave", function(arg)
            vimcode.message("left_insert:" .. arg)
        end)
        "#,
    );
    press(&mut e, 'i'); // Enter insert mode
    press_key(&mut e, "Escape"); // Leave insert mode
    assert!(
        e.message.contains("left_insert:Insert"),
        "InsertLeave should fire, got: {}",
        e.message
    );
}

#[test]
fn plugin_insert_enter_event_fires() {
    let mut e = engine_with_plugin(
        "hello\n",
        "insert_enter",
        r#"
        vimcode.on("InsertEnter", function(arg)
            vimcode.message("entered_insert:" .. arg)
        end)
        "#,
    );
    press(&mut e, 'i');
    assert!(
        e.message.contains("entered_insert:Insert"),
        "InsertEnter should fire, got: {}",
        e.message
    );
}

// ── BufWrite event ─────────────────────────────────────────────────────────

#[test]
fn plugin_buf_write_event_fires_on_save() {
    let tmp = std::env::temp_dir().join("vc_plugin_bufwrite_test.txt");
    std::fs::write(&tmp, "content\n").ok();

    let mut e = engine_with_plugin(
        "",
        "buf_write",
        r#"
        vimcode.on("BufWrite", function(path)
            vimcode.message("bufwrite:" .. path)
        end)
        "#,
    );
    e.open_file_in_tab(&tmp);
    let _ = e.save();
    assert!(
        e.message.contains("bufwrite:"),
        "BufWrite should fire on save, got: {}",
        e.message
    );

    let _ = std::fs::remove_file(&tmp);
}

// ── VimEnter event ─────────────────────────────────────────────────────────

#[test]
fn plugin_vim_enter_fires_on_init() {
    // VimEnter fires when init_plugins is called.
    // We test it by creating a plugin manager manually with a VimEnter hook.
    let dir = plugin_fixture_dir("vimenter");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("vimenter.lua"),
        r#"
        vimcode.on("VimEnter", function(_)
            vimcode.message("vim_started")
        end)
        "#,
    )
    .unwrap();

    let mut e = engine_with("");
    // Manually set up plugin loading to trigger VimEnter
    match vimcode_core::core::plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &[]);
            e.set_plugin_manager(mgr);
            // Fire VimEnter like init_plugins does
            e.plugin_event("VimEnter", "");
        }
        Err(_) => panic!("failed to create PluginManager"),
    }
    assert_eq!(e.message, "vim_started", "VimEnter should fire after init");
}

// ── BufNew / BufEnter events ───────────────────────────────────────────────

#[test]
fn plugin_buf_new_fires_on_open_file() {
    let tmp = std::env::temp_dir().join("vc_plugin_bufnew_test.txt");
    std::fs::write(&tmp, "new content\n").ok();

    let mut e = engine_with_plugin(
        "",
        "buf_new",
        r#"
        vimcode.on("BufNew", function(path)
            vimcode.message("bufnew:" .. path)
        end)
        "#,
    );
    e.open_file_in_tab(&tmp);
    assert!(
        e.message.contains("bufnew:"),
        "BufNew should fire on open, got: {}",
        e.message
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn plugin_buf_enter_fires_on_open_file() {
    let tmp = std::env::temp_dir().join("vc_plugin_bufenter_test.txt");
    std::fs::write(&tmp, "enter content\n").ok();

    let mut e = engine_with_plugin(
        "",
        "buf_enter",
        r#"
        vimcode.on("BufEnter", function(path)
            vimcode.message("bufenter:" .. path)
        end)
        "#,
    );
    e.open_file_in_tab(&tmp);
    assert!(
        e.message.contains("bufenter:"),
        "BufEnter should fire on open, got: {}",
        e.message
    );

    let _ = std::fs::remove_file(&tmp);
}

// ═══════════════════════════════════════════════════════════════════════════
// Native Commentary — gcc, gc (visual), :Comment / :Commentary
// (Commentary Lua extension removed; native comment toggling is built-in)
// ═══════════════════════════════════════════════════════════════════════════

/// Helper: engine with a known filetype set for comment detection.
fn engine_with_commentary(text: &str, lang: &str) -> vimcode_core::Engine {
    let mut e = engine_with(text);
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some(lang.to_string());
    }
    e
}

// ── :Commentary command ────────────────────────────────────────────────────

#[test]
fn commentary_command_comments_single_line_rust() {
    let mut e = engine_with_commentary("let x = 1;\nlet y = 2;\n", "rust");
    exec(&mut e, "Commentary");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// let x = 1;", "line should be commented");
    assert_eq!(lines[1], "let y = 2;", "second line untouched");
}

#[test]
fn commentary_command_uncomments_single_line_rust() {
    let mut e = engine_with_commentary("// let x = 1;\nlet y = 2;\n", "rust");
    exec(&mut e, "Commentary");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "let x = 1;", "line should be uncommented");
}

#[test]
fn commentary_command_with_count_comments_multiple_lines() {
    let mut e = engine_with_commentary("aaa\nbbb\nccc\n", "python");
    exec(&mut e, "Commentary 2");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "# aaa");
    assert_eq!(lines[1], "# bbb");
    assert_eq!(lines[2], "ccc", "third line untouched");
}

#[test]
fn commentary_preserves_indentation() {
    let mut e = engine_with_commentary("    let x = 1;\n", "rust");
    exec(&mut e, "Commentary");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "    // let x = 1;", "indent should be preserved");
}

#[test]
fn commentary_uncomments_with_indent() {
    let mut e = engine_with_commentary("    // let x = 1;\n", "rust");
    exec(&mut e, "Commentary");
    let lines = get_lines(&e);
    assert_eq!(
        lines[0], "    let x = 1;",
        "uncomment should preserve indent"
    );
}

#[test]
fn commentary_skips_blank_lines() {
    let mut e = engine_with_commentary("aaa\n\nbbb\n", "python");
    exec(&mut e, "Commentary 3");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "# aaa");
    assert_eq!(lines[1], "", "blank line stays blank");
    assert_eq!(lines[2], "# bbb");
}

#[test]
fn commentary_python_uses_hash() {
    let mut e = engine_with_commentary("x = 1\n", "python");
    exec(&mut e, "Commentary");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "# x = 1");
}

#[test]
fn commentary_lua_uses_double_dash() {
    let mut e = engine_with_commentary("local x = 1\n", "lua");
    exec(&mut e, "Commentary");
    let lines = get_lines(&e);
    assert_eq!(lines[0], "-- local x = 1");
}

// ── gcc key binding ────────────────────────────────────────────────────────

#[test]
fn gcc_comments_current_line() {
    let mut e = engine_with_commentary("let x = 1;\nlet y = 2;\n", "rust");
    // gcc = g, c, c
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// let x = 1;", "gcc should comment current line");
    assert_eq!(lines[1], "let y = 2;", "second line untouched");
}

#[test]
fn gcc_uncomments_commented_line() {
    let mut e = engine_with_commentary("// let x = 1;\n", "rust");
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "let x = 1;", "gcc should uncomment");
}

#[test]
fn gcc_with_count_comments_multiple_lines() {
    let mut e = engine_with_commentary("aaa\nbbb\nccc\nddd\n", "python");
    // 3gcc
    press(&mut e, '3');
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "# aaa");
    assert_eq!(lines[1], "# bbb");
    assert_eq!(lines[2], "# ccc");
    assert_eq!(lines[3], "ddd", "fourth line untouched");
}

#[test]
fn gcc_undo_restores_original_line() {
    let mut e = engine_with_commentary("let x = 1;\nlet y = 2;\n", "rust");
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// let x = 1;", "gcc should comment");

    // Undo
    press(&mut e, 'u');
    let lines2 = get_lines(&e);
    assert_eq!(lines2[0], "let x = 1;", "undo should restore original");
    assert!(
        !e.message.contains("oldest change"),
        "should not say 'oldest change': {}",
        e.message
    );
}

#[test]
fn gc_visual_undo_restores_original_lines() {
    let mut e = engine_with_commentary("aaa\nbbb\nccc\n", "rust");
    // Select two lines with V, then gc
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'g');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// aaa");
    assert_eq!(lines[1], "// bbb");

    // Undo
    press(&mut e, 'u');
    let lines2 = get_lines(&e);
    assert_eq!(lines2[0], "aaa", "undo should restore first line");
    assert_eq!(lines2[1], "bbb", "undo should restore second line");
    assert!(
        !e.message.contains("oldest change"),
        "should not say 'oldest change': {}",
        e.message
    );
}

// ── gc in visual mode ──────────────────────────────────────────────────────

#[test]
fn gc_visual_comments_selection() {
    let mut e = engine_with_commentary("aaa\nbbb\nccc\nddd\n", "rust");
    // Set language ID on buffer for engine-level toggle_comment_range
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some("rust".to_string());
    }
    // Select lines 1-3 with V (visual line mode)
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'j');
    // gc
    press(&mut e, 'g');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "// aaa");
    assert_eq!(lines[1], "// bbb");
    assert_eq!(lines[2], "// ccc");
    assert_eq!(lines[3], "ddd", "line 4 untouched");
    // Should be back in normal mode
    assert_eq!(e.mode, vimcode_core::Mode::Normal);
}

#[test]
fn gc_visual_uncomments_all_commented_lines() {
    let mut e = engine_with_commentary("// aaa\n// bbb\nccc\n", "rust");
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some("rust".to_string());
    }
    // Select first two lines
    press(&mut e, 'V');
    press(&mut e, 'j');
    // gc
    press(&mut e, 'g');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    assert_eq!(lines[0], "aaa", "should be uncommented");
    assert_eq!(lines[1], "bbb", "should be uncommented");
    assert_eq!(lines[2], "ccc", "untouched");
}

#[test]
fn gc_visual_mixed_comments_all() {
    let mut e = engine_with_commentary("// aaa\nbbb\n", "rust");
    let buf_id = e.active_buffer_id();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.lsp_language_id = Some("rust".to_string());
    }
    // Select both lines
    press(&mut e, 'V');
    press(&mut e, 'j');
    press(&mut e, 'g');
    press(&mut e, 'c');
    let lines = get_lines(&e);
    // Mixed (one commented, one not) — all get commented
    assert_eq!(
        lines[0], "// // aaa",
        "already-commented gets double prefix"
    );
    assert_eq!(lines[1], "// bbb", "uncommented gets prefix");
}

// ── Commentary is undoable ─────────────────────────────────────────────────

#[test]
fn gcc_is_undoable() {
    let mut e = engine_with_commentary("let x = 1;\n", "rust");
    press(&mut e, 'g');
    press(&mut e, 'c');
    press(&mut e, 'c');
    assert_eq!(get_lines(&e)[0], "// let x = 1;");
    // Undo
    press(&mut e, 'u');
    assert_eq!(get_lines(&e)[0], "let x = 1;", "undo should restore");
}

// ── Git API Lua bindings ────────────────────────────────────────────────────

use vimcode_core::core::plugin::{PluginCallContext, PluginManager};

/// A scratch plugin directory that is unique to this call.
///
/// This used to be keyed on `code.len()`, which is not a discriminator at all:
/// two scripts of the same length share a directory. `git_api_stash_list_returns_table`
/// and `git_api_blame_file_returns_table` both hashed to `vc_git_api_test_174`, so
/// under cargo's parallel test threads one test's `remove_dir_all` could delete the
/// other's `test.lua` in the window before `load_plugins_dir` read it. `read_dir`
/// then fails silently (see `PluginManager::load_plugins_dir`), no command is
/// registered, and the victim's `assert!(found)` panics — a harness race that reads
/// like a real regression.
///
/// Keying on pid + a monotonic counter makes every call site its own directory, so
/// no two tests can ever contend for one path.
fn plugin_test_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "vc_git_api_test_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Load `code` as a one-file plugin, returning the manager and the scratch
/// directory it was loaded from (so tests can assert on path uniqueness).
fn plugin_with_dir(code: &str) -> (PluginManager, std::path::PathBuf) {
    let dir = plugin_test_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.lua");
    std::fs::write(&path, code).unwrap();
    let mut pm = PluginManager::new().unwrap();
    pm.load_plugins_dir(&dir, &[]);
    // `load_plugins_dir` reads and executes eagerly, so the files are no longer
    // needed once it returns — clean up rather than leaking a dir per call.
    let _ = std::fs::remove_dir_all(&dir);
    (pm, dir)
}

fn plugin_with(code: &str) -> PluginManager {
    plugin_with_dir(code).0
}

#[test]
fn plugin_with_uses_a_distinct_dir_for_same_length_scripts() {
    // Regression guard for the flake described on `plugin_test_dir`. These two
    // scripts are deliberately the same byte length — the exact condition that
    // aliased two tests onto `/tmp/vc_git_api_test_174` and let one test's
    // `remove_dir_all` race the other's `load_plugins_dir`.
    //
    // Against the old `code.len()` keying this assertion is deterministically
    // red: both calls resolve to the identical path. Verified by reinstating
    // the old scheme locally before committing.
    let a = "vimcode.command(\"TestAaa\", function(_) vimcode.message(\"a\") end)";
    let b = "vimcode.command(\"TestBbb\", function(_) vimcode.message(\"b\") end)";
    assert_eq!(a.len(), b.len(), "fixture scripts must be the same length");

    let (pm_a, dir_a) = plugin_with_dir(a);
    let (pm_b, dir_b) = plugin_with_dir(b);
    assert_ne!(
        dir_a, dir_b,
        "same-length scripts must not share a scratch dir"
    );

    // …and both plugins really did load from their own directory.
    let (found_a, ctx_a) = pm_a.call_command("TestAaa", "", PluginCallContext::default());
    assert!(found_a, "first plugin's command should be registered");
    assert_eq!(ctx_a.message.as_deref(), Some("a"));

    let (found_b, ctx_b) = pm_b.call_command("TestBbb", "", PluginCallContext::default());
    assert!(found_b, "second plugin's command should be registered");
    assert_eq!(ctx_b.message.as_deref(), Some("b"));
}

#[test]
fn plugin_scratch_dirs_are_cleaned_up() {
    // The old helper left one dir per distinct script length behind in /tmp;
    // the unique-path scheme would leak one per call if we did not clean up.
    let (_pm, dir) = plugin_with_dir("vimcode.command(\"TestTmp\", function(_) end)");
    assert!(!dir.exists(), "scratch dir {dir:?} should be removed");
}

#[test]
fn git_api_show_returns_nil_without_cwd() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestShow", function(args)
            local result = vimcode.git.show(args)
            if result then
                vimcode.message("got: " .. string.sub(result, 1, 20))
            else
                vimcode.message("nil")
            end
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestShow", "HEAD", ctx);
    assert!(found);
    assert_eq!(ctx.message.as_deref(), Some("nil"));
}

#[test]
fn git_api_show_returns_content_for_valid_hash() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestShow", function(args)
            local result = vimcode.git.show(args)
            if result then
                vimcode.message("ok")
            else
                vimcode.message("nil")
            end
        end)
    "#,
    );
    // Use the actual cwd (this project's repo)
    let cwd = std::env::current_dir().unwrap();
    let ctx = PluginCallContext {
        cwd_path: Some(cwd),
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestShow", "HEAD", ctx);
    assert!(found);
    assert_eq!(ctx.message.as_deref(), Some("ok"));
}

#[test]
fn git_api_repo_root_returns_path() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestRoot", function(_)
            local root = vimcode.git.repo_root()
            if root then
                vimcode.message("root:" .. root)
            else
                vimcode.message("nil")
            end
        end)
    "#,
    );
    let cwd = std::env::current_dir().unwrap();
    let ctx = PluginCallContext {
        cwd_path: Some(cwd.clone()),
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestRoot", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap();
    assert!(msg.starts_with("root:"), "expected root prefix, got {msg}");
    let root_path = msg.strip_prefix("root:").unwrap();
    // Verify the returned path is a real directory (not hardcoding a specific
    // path segment, since the repo may be checked out in a worktree at an
    // arbitrary location such as .coord/worktrees/<hash>).
    assert!(
        std::path::Path::new(root_path).is_dir(),
        "repo root should be an existing directory: {root_path}"
    );
}

#[test]
fn git_api_branch_returns_current_branch() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestBranch", function(_)
            local b = vimcode.git.branch()
            if b then
                vimcode.message("branch:" .. b)
            else
                vimcode.message("nil")
            end
        end)
    "#,
    );
    let cwd = std::env::current_dir().unwrap();
    let ctx = PluginCallContext {
        cwd_path: Some(cwd),
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestBranch", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap();
    // In CI detached HEAD, git branch --show-current returns empty → Lua gets nil
    assert!(
        msg.starts_with("branch:") || msg == "nil",
        "expected branch prefix or nil (detached HEAD), got {msg}"
    );
}

#[test]
fn git_api_diff_ref_returns_nil_without_cwd() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestDiff", function(args)
            local result = vimcode.git.diff_ref(args)
            if result then
                vimcode.message("got diff")
            else
                vimcode.message("nil")
            end
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestDiff", "HEAD", ctx);
    assert!(found);
    assert_eq!(ctx.message.as_deref(), Some("nil"));
}

#[test]
fn git_api_log_returns_entries() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestLog", function(_)
            local entries = vimcode.git.log(5)
            vimcode.message("count:" .. #entries)
        end)
    "#,
    );
    let cwd = std::env::current_dir().unwrap();
    let ctx = PluginCallContext {
        cwd_path: Some(cwd),
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestLog", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap();
    assert!(
        msg.starts_with("count:"),
        "expected count prefix, got {msg}"
    );
    let count: usize = msg.strip_prefix("count:").unwrap().parse().unwrap();
    assert!(count > 0, "should have at least 1 log entry");
    assert!(count <= 5, "should have at most 5 entries");
}

#[test]
fn git_api_stash_list_returns_table() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestStashList", function(_)
            local entries = vimcode.git.stash_list()
            vimcode.message("count:" .. #entries)
        end)
    "#,
    );
    let cwd = std::env::current_dir().unwrap();
    let ctx = PluginCallContext {
        cwd_path: Some(cwd),
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestStashList", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap();
    assert!(
        msg.starts_with("count:"),
        "expected count prefix, got {msg}"
    );
    // Stash list can be empty, just verify it returns a number
    let _count: usize = msg.strip_prefix("count:").unwrap().parse().unwrap();
}

#[test]
fn git_api_stash_push_without_cwd_returns_error() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestStashPush", function(args)
            local result = vimcode.git.stash_push(args)
            vimcode.message(result)
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestStashPush", "test msg", ctx);
    assert!(found);
    assert_eq!(
        ctx.message.as_deref(),
        Some("no working directory"),
        "should return error without cwd"
    );
}

#[test]
fn git_api_stash_show_without_cwd_returns_nil() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestStashShow", function(_)
            local result = vimcode.git.stash_show(0)
            if result then
                vimcode.message("got diff")
            else
                vimcode.message("nil")
            end
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestStashShow", "", ctx);
    assert!(found);
    assert_eq!(ctx.message.as_deref(), Some("nil"));
}

#[test]
fn git_api_blame_file_returns_table() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestBlameFile", function(_)
            local entries = vimcode.git.blame_file()
            vimcode.message("count:" .. #entries)
        end)
    "#,
    );
    // Point at a real file in this repo
    let cwd = std::env::current_dir().unwrap();
    let file = cwd.join("Cargo.toml");
    let ctx = PluginCallContext {
        cwd_path: Some(cwd),
        buf_path_os: Some(file),
        buf_dirty: false,
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestBlameFile", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap();
    assert!(
        msg.starts_with("count:"),
        "expected count prefix, got {msg}"
    );
    let count: usize = msg.strip_prefix("count:").unwrap().parse().unwrap();
    assert!(count > 0, "Cargo.toml should have blame entries");
}

#[test]
fn git_api_file_log_detailed_returns_entries() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestFileLogDetailed", function(_)
            local entries = vimcode.git.file_log_detailed(5)
            if #entries > 0 then
                local e = entries[1]
                vimcode.message("hash:" .. e.hash .. " author:" .. e.author)
            else
                vimcode.message("empty")
            end
        end)
    "#,
    );
    let cwd = std::env::current_dir().unwrap();
    let file = cwd.join("Cargo.toml");
    let ctx = PluginCallContext {
        cwd_path: Some(cwd),
        buf_path_os: Some(file),
        buf_dirty: false,
        ..Default::default()
    };
    let (found, ctx) = pm.call_command("TestFileLogDetailed", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap();
    assert!(msg.starts_with("hash:"), "expected hash prefix, got {msg}");
    assert!(msg.contains("author:"), "should contain author field");
}

#[test]
fn git_api_line_log_returns_nil_without_file() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestLineLog", function(_)
            local entries = vimcode.git.line_log(1, 1, 5)
            vimcode.message("count:" .. #entries)
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestLineLog", "", ctx);
    assert!(found);
    assert_eq!(ctx.message.as_deref(), Some("count:0"));
}

// ── Scratch buffer API tests ────────────────────────────────────────────────

#[test]
fn scratch_buffer_basic_open() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestScratch", function(_)
            vimcode.buf.open_scratch("TestBuf", "hello\nworld", {
                readonly = true,
            })
            vimcode.message("ok")
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestScratch", "", ctx);
    assert!(found);
    assert_eq!(ctx.message.as_deref(), Some("ok"));
    assert_eq!(ctx.scratch_buffers.len(), 1);
    assert_eq!(ctx.scratch_buffers[0].name, "TestBuf");
    assert_eq!(ctx.scratch_buffers[0].content, "hello\nworld");
    assert!(ctx.scratch_buffers[0].read_only);
    assert!(ctx.scratch_buffers[0].filetype.is_none());
    assert!(ctx.scratch_buffers[0].split.is_none());
}

#[test]
fn scratch_buffer_with_filetype_and_split() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestScratchOpts", function(_)
            vimcode.buf.open_scratch("DiffBuf", "--- a\n+++ b", {
                readonly = true,
                filetype = "diff",
                split = "vertical",
            })
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestScratchOpts", "", ctx);
    assert!(found);
    assert_eq!(ctx.scratch_buffers.len(), 1);
    assert_eq!(ctx.scratch_buffers[0].filetype.as_deref(), Some("diff"));
    assert_eq!(ctx.scratch_buffers[0].split.as_deref(), Some("vertical"));
}

#[test]
fn scratch_buffer_defaults_to_readonly() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestDefaults", function(_)
            vimcode.buf.open_scratch("Buf", "content", nil)
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestDefaults", "", ctx);
    assert!(found);
    assert_eq!(ctx.scratch_buffers.len(), 1);
    assert!(
        ctx.scratch_buffers[0].read_only,
        "default should be readonly"
    );
}

#[test]
fn scratch_buffer_writable() {
    let pm = plugin_with(
        r#"
        vimcode.command("TestWritable", function(_)
            vimcode.buf.open_scratch("Edit", "text", { readonly = false })
        end)
    "#,
    );
    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TestWritable", "", ctx);
    assert!(found);
    assert!(!ctx.scratch_buffers[0].read_only);
}

#[test]
fn scratch_buffer_engine_creates_buffer() {
    let mut e = engine_with("original content\n");
    // Simulate what apply_plugin_ctx does — call a command that opens a scratch
    run_cmd(&mut e, "GitRepoLog");
    // The command won't actually open because git-insights scripts aren't loaded
    // in the test engine. Instead test via direct engine manipulation.
    let buf_id = e.buffer_manager.create();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.buffer.content = ropey::Rope::from_str("scratch content\n");
        state.scratch_name = Some("TestScratch".to_string());
        state.read_only = true;
    }
    let state = e.buffer_manager.get(buf_id).unwrap();
    assert_eq!(state.display_name(), "[TestScratch]");
    assert!(state.read_only);
    assert_eq!(state.buffer.content.to_string(), "scratch content\n");
}

#[test]
fn scratch_buffer_display_name() {
    let mut e = engine_with("");
    let buf_id = e.buffer_manager.create();
    if let Some(state) = e.buffer_manager.get_mut(buf_id) {
        state.scratch_name = Some("GitFileHistory".to_string());
    }
    assert_eq!(
        e.buffer_manager.get(buf_id).unwrap().display_name(),
        "[GitFileHistory]"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Extension versioning, cross-platform, backward compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extension_version_stored_on_install() {
    use vimcode_core::core::extensions::*;
    let mut e = engine_with("");
    e.ext_registry = Some(vec![ExtensionManifest {
        name: "test-ext".to_string(),
        display_name: "Test Extension".to_string(),
        version: "1.2.3".to_string(),
        scripts: vec![],
        ..Default::default()
    }]);
    e.ext_install_from_registry("test-ext");
    assert!(e.extension_state.is_installed("test-ext"));
    assert_eq!(e.extension_state.installed_version("test-ext"), "1.2.3");
    // Clean up
    e.extension_state.installed.clear();
}

#[test]
fn extension_version_update_detection() {
    use vimcode_core::core::extensions::*;
    let mut e = engine_with("");
    // Install at v1.0.0
    e.extension_state.mark_installed_version("my-ext", "1.0.0");
    // Registry has v2.0.0
    e.ext_registry = Some(vec![ExtensionManifest {
        name: "my-ext".to_string(),
        display_name: "My Extension".to_string(),
        version: "2.0.0".to_string(),
        ..Default::default()
    }]);
    assert!(
        e.ext_has_update("my-ext"),
        "should detect update when versions differ"
    );
    // Update to v2.0.0
    e.extension_state.mark_installed_version("my-ext", "2.0.0");
    assert!(
        !e.ext_has_update("my-ext"),
        "should not detect update when versions match"
    );
    // Clean up
    e.extension_state.installed.clear();
}

#[test]
fn extension_version_no_update_for_uninstalled() {
    use vimcode_core::core::extensions::*;
    let mut e = engine_with("");
    e.ext_registry = Some(vec![ExtensionManifest {
        name: "not-installed".to_string(),
        display_name: "Not Installed".to_string(),
        version: "1.0.0".to_string(),
        ..Default::default()
    }]);
    assert!(
        !e.ext_has_update("not-installed"),
        "uninstalled extension should not report update"
    );
}

#[test]
fn extension_version_empty_registry_version_no_update() {
    use vimcode_core::core::extensions::*;
    let mut e = engine_with("");
    e.extension_state.mark_installed_version("my-ext", "1.0.0");
    // Registry has empty version
    e.ext_registry = Some(vec![ExtensionManifest {
        name: "my-ext".to_string(),
        display_name: "My Extension".to_string(),
        version: String::new(),
        ..Default::default()
    }]);
    assert!(
        !e.ext_has_update("my-ext"),
        "empty registry version should not trigger update"
    );
    e.extension_state.installed.clear();
}

#[test]
fn extension_state_backward_compat_plain_strings() {
    // Old extensions.json format: installed is a plain string list
    let json = r#"{"installed":["rust","python"],"dismissed":["java"]}"#;
    let state: vimcode_core::core::session::ExtensionState =
        serde_json::from_str(json).expect("should deserialize old format");
    assert!(state.is_installed("rust"));
    assert!(state.is_installed("python"));
    assert!(state.is_dismissed("java"));
    // Version should be empty for migrated entries
    assert_eq!(state.installed_version("rust"), "");
    assert_eq!(state.installed_version("python"), "");
}

#[test]
fn extension_state_new_format_roundtrip() {
    let json = r#"{"installed":[{"name":"rust","version":"1.0.0"},{"name":"python","version":"2.1.0"}],"dismissed":[]}"#;
    let state: vimcode_core::core::session::ExtensionState =
        serde_json::from_str(json).expect("should deserialize new format");
    assert!(state.is_installed("rust"));
    assert_eq!(state.installed_version("rust"), "1.0.0");
    assert_eq!(state.installed_version("python"), "2.1.0");
    // Re-serialize and back
    let json2 = serde_json::to_string(&state).unwrap();
    let state2: vimcode_core::core::session::ExtensionState =
        serde_json::from_str(&json2).expect("roundtrip should work");
    assert_eq!(state2.installed_version("rust"), "1.0.0");
}

#[test]
fn extension_state_mixed_format() {
    // Mixed: some old strings, some new objects
    let json = r#"{"installed":["old-ext",{"name":"new-ext","version":"3.0.0"}],"dismissed":[]}"#;
    let state: vimcode_core::core::session::ExtensionState =
        serde_json::from_str(json).expect("should handle mixed format");
    assert!(state.is_installed("old-ext"));
    assert_eq!(state.installed_version("old-ext"), "");
    assert!(state.is_installed("new-ext"));
    assert_eq!(state.installed_version("new-ext"), "3.0.0");
}

#[test]
fn extension_mark_installed_version_updates_existing() {
    let mut state = vimcode_core::core::session::ExtensionState::default();
    state.mark_installed_version("ext-a", "1.0.0");
    assert_eq!(state.installed_version("ext-a"), "1.0.0");
    // Update version
    state.mark_installed_version("ext-a", "2.0.0");
    assert_eq!(state.installed_version("ext-a"), "2.0.0");
    // Should still be a single entry
    assert_eq!(
        state.installed.iter().filter(|e| e.name == "ext-a").count(),
        1
    );
}

#[test]
fn platform_install_cmd_fallback() {
    use vimcode_core::core::extensions::*;
    // When platform-specific fields are empty, falls back to generic install
    let lsp = LspConfig {
        install: "npm install -g foo".to_string(),
        ..Default::default()
    };
    assert_eq!(lsp.install_cmd_for_platform(), "npm install -g foo");
}

#[test]
fn platform_install_cmd_linux_override() {
    use vimcode_core::core::extensions::*;
    let lsp = LspConfig {
        install: "generic-cmd".to_string(),
        install_linux: "linux-cmd".to_string(),
        ..Default::default()
    };
    // On Linux, the platform-specific command should be preferred
    #[cfg(target_os = "linux")]
    assert_eq!(lsp.install_cmd_for_platform(), "linux-cmd");
    #[cfg(target_os = "macos")]
    assert_eq!(lsp.install_cmd_for_platform(), "generic-cmd");
    #[cfg(target_os = "windows")]
    assert_eq!(lsp.install_cmd_for_platform(), "generic-cmd");
}

#[test]
fn dap_platform_install_cmd_fallback() {
    use vimcode_core::core::extensions::*;
    let dap = DapConfig {
        install: "install-dap".to_string(),
        ..Default::default()
    };
    assert_eq!(dap.install_cmd_for_platform(), "install-dap");
}

#[test]
fn ext_update_not_installed() {
    let mut e = engine_with("");
    e.ext_registry = Some(test_manifests());
    e.ext_update_one("bash");
    assert!(
        e.message.contains("not installed"),
        "should report not installed: {}",
        e.message
    );
}

#[test]
fn ext_update_all_when_up_to_date() {
    use vimcode_core::core::extensions::*;
    let mut e = engine_with("");
    // Install with matching version
    e.extension_state.mark_installed_version("bash", "1.0.0");
    e.ext_registry = Some(vec![ExtensionManifest {
        name: "bash".to_string(),
        display_name: "Bash".to_string(),
        version: "1.0.0".to_string(),
        ..Default::default()
    }]);
    e.ext_update_all();
    assert!(
        e.message.contains("up to date"),
        "should report up to date: {}",
        e.message
    );
    e.extension_state.installed.clear();
}

#[test]
fn config_dir_helper_returns_vimcode() {
    let dir = vimcode_core::core::paths::vimcode_config_dir();
    let s = dir.to_string_lossy();
    assert!(
        s.contains("vimcode"),
        "config dir should contain 'vimcode': {s}"
    );
}

// ---------------------------------------------------------------------------
// #917: macOS Homebrew LSP-binary resolution
// ---------------------------------------------------------------------------
//
// `resolve_command` probes a fixed set of directories before falling back to
// `which` (which inherits this process's PATH). On macOS neither
// `/opt/homebrew/bin` nor `/usr/local/bin` was ever in that list, so a
// Homebrew-installed language server (7 of 19 registry extensions install
// via Homebrew) installed successfully and then was never found when
// vimcode ran as a native `.app` bundle with launchd's minimal PATH.
//
// These tests can't touch real `/opt/homebrew` from a Linux box (and
// shouldn't touch it even on a real Mac), so they drive the exact same
// resolution path through `VIMCODE_TEST_HOMEBREW_PREFIXES` — an override
// `resolve_command`'s `homebrew_prefixes()` honours on *every* target,
// substituting a fake prefix for the real ones. That way this behaviour is
// exercised on every CI host rather than going untested forever because this
// repo's CI has no macOS runner.
//
// The override was originally `cfg(not(macos))`-gated, which made both tests
// unconditionally fail when the suite ran on an actual Mac: the fake prefix
// was ignored and `clangd` resolved to the host's `/usr/bin/clangd`. Keep it
// ungated — a test that only passes on the OS the feature *isn't* for is not
// coverage.
//
// A `Mutex` serializes the two tests below since both mutate process-global
// `PATH` / `VIMCODE_TEST_HOMEBREW_PREFIXES` state; an RAII guard restores
// both on drop (including on panic) so they can't leak into other tests.
static HOMEBREW_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard: snapshot + restore an environment variable across a test.
struct EnvVarGuard {
    key: &'static str,
    old: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let old = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, old }
    }

    /// Remove `key` for the lifetime of the guard, restoring whatever was
    /// there before on drop. Used to assert *default* behaviour for an
    /// opt-in env var when the ambient environment might already set it.
    fn unset(key: &'static str) -> Self {
        let old = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.old.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

#[test]
fn resolve_command_finds_binary_under_fake_homebrew_prefix() {
    let _lock = HOMEBREW_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let prefix =
        std::env::temp_dir().join(format!("vimcode_test_brew_prefix_{}", std::process::id()));
    let bin_dir = prefix.join("bin");
    let _ = std::fs::remove_dir_all(&prefix);
    std::fs::create_dir_all(&bin_dir).unwrap();
    // Uniquely named so this can't accidentally resolve via some unrelated
    // binary that happens to already sit on the test host's real PATH.
    let binary_name = "vimcode-test-fake-lsp-917";
    let binary_path = bin_dir.join(binary_name);
    std::fs::write(&binary_path, "#!/bin/sh\necho fake\n").unwrap();

    let _prefix_guard = EnvVarGuard::set("VIMCODE_TEST_HOMEBREW_PREFIXES", prefix.as_os_str());
    // launchd's PATH for a GUI-launched app has no Homebrew prefix on it —
    // simulate that exactly, so this test cannot pass merely because the
    // developer's login shell (or CI runner) has brew, or anything else, on
    // PATH already.
    let _path_guard = EnvVarGuard::set("PATH", std::ffi::OsStr::new("/usr/bin:/bin"));

    let resolved = vimcode_core::core::lsp_manager::resolve_command(binary_name);
    assert_eq!(
        resolved,
        Some(binary_path.clone()),
        "should resolve {binary_name} via the fake Homebrew prefix bin/ dir"
    );

    let _ = std::fs::remove_dir_all(&prefix);
}

#[test]
fn resolve_command_finds_keg_only_clangd_under_homebrew_opt() {
    let _lock = HOMEBREW_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Homebrew's `llvm` formula (which ships `clangd`) is keg-only — its
    // binaries are never symlinked into `<prefix>/bin`, only into
    // `<prefix>/opt/llvm/bin`. Deliberately leave `bin/` empty here so this
    // only passes if resolution goes through the keg-only probe, not the
    // ordinary `<prefix>/bin` lookup.
    let prefix =
        std::env::temp_dir().join(format!("vimcode_test_brew_kegonly_{}", std::process::id()));
    let kegged_bin_dir = prefix.join("opt").join("llvm").join("bin");
    let _ = std::fs::remove_dir_all(&prefix);
    std::fs::create_dir_all(&kegged_bin_dir).unwrap();
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    let clangd_path = kegged_bin_dir.join("clangd");
    std::fs::write(&clangd_path, "#!/bin/sh\necho fake\n").unwrap();

    let _prefix_guard = EnvVarGuard::set("VIMCODE_TEST_HOMEBREW_PREFIXES", prefix.as_os_str());
    let _path_guard = EnvVarGuard::set("PATH", std::ffi::OsStr::new("/usr/bin:/bin"));

    let resolved = vimcode_core::core::lsp_manager::resolve_command("clangd");
    assert_eq!(
        resolved,
        Some(clangd_path.clone()),
        "clangd should resolve via <prefix>/opt/llvm/bin since llvm is keg-only"
    );

    let _ = std::fs::remove_dir_all(&prefix);
}

// ---------------------------------------------------------------------------
// #918: extension install failures must name a runnable, platform-correct fix
// ---------------------------------------------------------------------------
//
// Confirmed RED against unfixed `develop` before these were added: the
// missing-dependency message was `"{name} requires npm — install npm and
// try again"` (no runnable command at all — asserting `.contains("brew")` or
// `.contains("apt")` failed), and the empty-install-command path returned
// `None` from `ensure_server_for_language` without ever touching
// `last_start_error`, so `mgr.last_start_error` stayed `None` (the
// `.is_some()` assertion below failed).

#[test]
fn missing_dependency_error_includes_runnable_command_for_platform() {
    // Defect 1: naming the missing binary ("requires npm") is not
    // actionable on its own. Drives `missing_dependency_message` directly
    // (a pure function with no PATH access) rather than going through
    // `ensure_server_for_language`'s live `resolve_command` check, so this
    // doesn't depend on whether npm/dotnet/go/etc. happen to be installed
    // on whatever machine runs the test suite (see the Homebrew tests above
    // for the same concern in a different corner of this file).
    use vimcode_core::core::extensions::{ExtensionManifest, LspConfig};
    use vimcode_core::core::lsp_manager::missing_dependency_message;

    let manifest = ExtensionManifest {
        name: "bash".to_string(),
        display_name: "Bash / Shell Support".to_string(),
        lsp: LspConfig {
            binary: "bash-language-server".to_string(),
            install: "npm install -g bash-language-server".to_string(),
            dependencies: vec!["npm".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };

    let msg = missing_dependency_message(&manifest, &["npm"]);
    assert!(
        msg.contains("Bash / Shell Support") && msg.contains("npm"),
        "message should still name the extension and the missing binary: {msg}"
    );
    // The real regression: the message must contain a *runnable command*,
    // not just the bare binary name repeated back.
    #[cfg(target_os = "linux")]
    assert!(
        msg.contains("sudo apt install nodejs npm"),
        "expected a runnable Linux install command: {msg}"
    );
    #[cfg(target_os = "macos")]
    assert!(
        msg.contains("brew install node"),
        "expected a runnable macOS install command: {msg}"
    );
    #[cfg(target_os = "windows")]
    assert!(
        msg.contains("winget install OpenJS.NodeJS"),
        "expected a runnable Windows install command: {msg}"
    );
}

#[test]
fn missing_dependency_error_falls_back_generically_for_unknown_prereq() {
    // A dependency name outside the built-in prereq table (#918's
    // `PREREQ_INSTALLS`) should still produce a message — just without a
    // specific command, since none is known.
    use vimcode_core::core::extensions::ExtensionManifest;
    use vimcode_core::core::lsp_manager::missing_dependency_message;

    let manifest = ExtensionManifest {
        name: "obscure".to_string(),
        display_name: "Obscure Extension".to_string(),
        ..Default::default()
    };
    let msg = missing_dependency_message(&manifest, &["some-obscure-tool"]);
    assert!(
        msg.contains("Obscure Extension") && msg.contains("some-obscure-tool"),
        "fallback message should still name the extension and dependency: {msg}"
    );
}

#[test]
fn ensure_server_for_language_names_extension_when_no_install_command() {
    // Defect 2: when a matching extension manifest has no install command
    // (empty `install`/`install_*` AND/OR empty `lsp.binary`) for this
    // platform, `ensure_server_for_language` must still set
    // `last_start_error` — never silently `return None` with nothing set.
    use vimcode_core::core::extensions::ExtensionManifest;
    use vimcode_core::core::lsp_manager::LspManager;

    let manifest = ExtensionManifest {
        name: "no-installer-lang".to_string(),
        display_name: "No Installer Extension".to_string(),
        language_ids: vec!["no-installer-lang".to_string()],
        // lsp left fully default: empty binary, empty install/install_*.
        // This is exactly the shape of the real `java` manifest before
        // #918 (no install path anywhere).
        ..Default::default()
    };

    let mut mgr = LspManager::new(std::env::temp_dir(), &[]);
    mgr.set_ext_manifests(vec![manifest.clone()], vec![manifest]);

    let result = mgr.ensure_server_for_language("no-installer-lang");
    assert!(
        result.is_none(),
        "no binary/install command means no server can start"
    );
    let err = mgr
        .last_start_error
        .clone()
        .expect("last_start_error must be set — never a silent dead end (#918)");
    assert!(
        err.contains("No Installer Extension"),
        "error should name the extension: {err}"
    );
}

#[test]
fn java_lsp_install_command_resolves_on_macos() {
    // Defect 3 concrete instance: `java` had no install command on any
    // platform, anywhere in its manifest — the one registry extension with
    // no install path at all. Confirm the macOS fill-in (`brew install
    // jdtls`, a real Homebrew-core formula) actually resolves through
    // `install_cmd_for_platform()`.
    use vimcode_core::core::extensions::find_manifest_by_name;
    let manifests = test_manifests();
    let java = find_manifest_by_name(&manifests, "java").expect("java manifest");

    #[cfg(target_os = "macos")]
    assert_eq!(
        java.lsp.install_cmd_for_platform(),
        "brew install jdtls",
        "java should resolve a non-empty macOS install command"
    );

    // Linux/Windows are deliberately deferred (#918) — no comparable
    // one-line package-manager install exists for eclipse-jdtls on either.
    // Confirm the macOS field itself is wired up regardless of which
    // platform runs this test, so it can't silently regress to empty.
    assert_eq!(
        java.lsp.install_macos, "brew install jdtls",
        "java's macOS install command must be set"
    );
}

// ---------------------------------------------------------------------------
// #919: registry conformance gate — every manifest resolves an install
// command on all three platforms
// ---------------------------------------------------------------------------
//
// Turns "every extension has a working install path on every platform" from
// an unverifiable 38-row checklist (19 extensions × {lsp, dap}) into a test
// that fails until it's true. `registry_conformance_snapshot()` below is a
// manual, point-in-time copy of the live registry (fetched from
// `JDonaghy/vimcode-ext`'s `registry.json`, trimmed to the `lsp`/`dap` fields
// this gate inspects — 19 extensions, matching the count in issue #919).
// It is intentionally NOT a live network fetch: tests must be hermetic and
// deterministic in CI. If the real registry changes, this snapshot needs a
// matching update — and per this gate's design, a regression introduced by
// that update fails loudly here instead of shipping silently.
//
// IMPORTANT: this snapshot is a distinct, independent copy of registry data
// from `test_manifests()` above. `test_manifests()`'s `java` entry carries an
// aspirational `install_macos = "brew install jdtls"` local-only fixup (from
// the #918 PR) that was never actually deployed to the live
// `vimcode-ext` registry — confirmed by re-fetching the real registry.json
// while building this gate. `registry_conformance_snapshot()`'s `java` entry
// deliberately reflects the *real* live state (no install command anywhere)
// so this gate's failure/allow-list matches reality, not the other fixture's
// aspiration.
fn registry_conformance_snapshot() -> Vec<vimcode_core::core::extensions::ExtensionManifest> {
    use vimcode_core::core::extensions::*;

    fn lsp(binary: &str, install: &str, linux: &str, macos: &str, windows: &str) -> LspConfig {
        LspConfig {
            binary: binary.to_string(),
            install: install.to_string(),
            install_linux: linux.to_string(),
            install_macos: macos.to_string(),
            install_windows: windows.to_string(),
            ..Default::default()
        }
    }

    fn dap(adapter: &str, binary: &str, install: &str) -> DapConfig {
        DapConfig {
            adapter: adapter.to_string(),
            binary: binary.to_string(),
            install: install.to_string(),
            ..Default::default()
        }
    }

    fn manifest(name: &str, lsp: LspConfig, dap: DapConfig) -> ExtensionManifest {
        ExtensionManifest {
            name: name.to_string(),
            display_name: name.to_string(),
            lsp,
            dap,
            ..Default::default()
        }
    }

    vec![
        manifest(
            "bash",
            lsp(
                "bash-language-server",
                "npm install -g bash-language-server",
                "",
                "",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "bicep",
            lsp(
                "bicep-langserver",
                "",
                r#"mkdir -p ~/.local/bin ~/.local/share/bicep-langserver && DOTNET_MAJOR=$(dotnet --version 2>/dev/null | cut -d. -f1); if [ "$DOTNET_MAJOR" -ge 10 ] 2>/dev/null; then BICEP_TAG=$(curl -s https://api.github.com/repos/Azure/bicep/releases/latest | grep tag_name | cut -d'"' -f4); else BICEP_TAG="v0.39.26"; fi; echo "Installing bicep-langserver $BICEP_TAG (dotnet $DOTNET_MAJOR)"; curl -sL "https://github.com/Azure/bicep/releases/download/$BICEP_TAG/bicep-langserver.zip" -o /tmp/bicep-langserver.zip && unzip -o /tmp/bicep-langserver.zip -d ~/.local/share/bicep-langserver && printf '#!/bin/sh\nexec dotnet ~/.local/share/bicep-langserver/Bicep.LangServer.dll "$@"\n' > ~/.local/bin/bicep-langserver && chmod +x ~/.local/bin/bicep-langserver && rm /tmp/bicep-langserver.zip"#,
                "brew install bicep && mkdir -p ~/.local/bin && ln -sf $(brew --prefix)/bin/bicep-langserver ~/.local/bin/bicep-langserver",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "cpp",
            lsp(
                "clangd",
                "",
                "sudo apt-get install -y clangd",
                "brew install llvm",
                "",
            ),
            dap("codelldb", "codelldb", ""),
        ),
        manifest(
            "csharp",
            lsp(
                "csharp-ls",
                "dotnet tool install -g csharp-ls",
                "",
                "",
                "",
            ),
            dap("netcoredbg", "netcoredbg", ""),
        ),
        manifest(
            "git-insights",
            LspConfig::default(),
            DapConfig::default(),
        ),
        manifest(
            "go",
            lsp(
                "gopls",
                "go install golang.org/x/tools/gopls@latest",
                "",
                "",
                "",
            ),
            dap(
                "delve",
                "dlv",
                "go install github.com/go-delve/delve/cmd/dlv@latest",
            ),
        ),
        manifest(
            "java",
            // Real live state (re-verified against the vimcode-ext registry
            // while building this gate): no install command anywhere. See
            // the module-level doc comment above for why this differs from
            // `test_manifests()`'s `java` entry.
            lsp("jdtls", "", "", "", ""),
            dap("java-debug", "java-debug-adapter", ""),
        ),
        manifest(
            "javascript",
            lsp(
                "typescript-language-server",
                "npm install -g typescript typescript-language-server",
                "",
                "",
                "",
            ),
            dap("js-debug", "node", ""),
        ),
        manifest(
            "latex",
            lsp(
                "texlab",
                "cargo install --git https://github.com/latex-lsp/texlab",
                "cargo install --git https://github.com/latex-lsp/texlab",
                "brew install texlab",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "json",
            lsp(
                "vscode-json-languageserver",
                "npm install -g vscode-langservers-extracted",
                "",
                "",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "lua",
            lsp(
                "lua-language-server",
                "",
                r#"mkdir -p ~/.local/share/lua-language-server ~/.local/bin && curl -sL "$(curl -s https://api.github.com/repos/LuaLS/lua-language-server/releases/latest | grep browser_download_url | grep linux-x64.tar.gz | head -1 | cut -d'"' -f4)" | tar xz -C ~/.local/share/lua-language-server && ln -sf ~/.local/share/lua-language-server/bin/lua-language-server ~/.local/bin/lua-language-server"#,
                "brew install lua-language-server",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "markdown",
            lsp(
                "marksman",
                "",
                r#"mkdir -p ~/.local/bin && curl -sL "$(curl -s https://api.github.com/repos/artempyanykh/marksman/releases/latest | grep browser_download_url | grep marksman-linux-x64 | head -1 | cut -d'"' -f4)" -o ~/.local/bin/marksman && chmod +x ~/.local/bin/marksman"#,
                "brew install marksman",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "php",
            lsp("intelephense", "npm install -g intelephense", "", "", ""),
            DapConfig::default(),
        ),
        manifest(
            "python",
            lsp(
                "pyright-langserver",
                "npm install -g pyright",
                "",
                "",
                "",
            ),
            dap("debugpy", "python", ""),
        ),
        manifest(
            "ruby",
            lsp("ruby-lsp", "gem install ruby-lsp", "", "", ""),
            DapConfig::default(),
        ),
        manifest(
            "rust",
            lsp(
                "rust-analyzer",
                "rustup component add rust-analyzer",
                "",
                "",
                "rustup component add rust-analyzer",
            ),
            dap("codelldb", "codelldb", ""),
        ),
        manifest(
            "terraform",
            lsp(
                "terraform-ls",
                "",
                r#"mkdir -p ~/.local/bin && curl -sL "$(curl -s https://api.github.com/repos/hashicorp/terraform-ls/releases/latest | grep browser_download_url | grep linux_amd64.zip | head -1 | cut -d'"' -f4)" -o /tmp/terraform-ls.zip && unzip -o /tmp/terraform-ls.zip terraform-ls -d ~/.local/bin && chmod +x ~/.local/bin/terraform-ls && rm /tmp/terraform-ls.zip"#,
                "brew install hashicorp/tap/terraform-ls",
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "xml",
            lsp(
                "lemminx",
                "",
                r#"mkdir -p ~/.local/bin && curl -sL "$(curl -s https://api.github.com/repos/eclipse/lemminx/releases/latest | grep browser_download_url | grep linux-x86_64 | grep -v sha256 | head -1 | cut -d'"' -f4)" -o ~/.local/bin/lemminx && chmod +x ~/.local/bin/lemminx"#,
                r#"mkdir -p ~/.local/bin && curl -sL "$(curl -s https://api.github.com/repos/eclipse/lemminx/releases/latest | grep browser_download_url | grep osx-x86_64 | grep -v sha256 | head -1 | cut -d'"' -f4)" -o ~/.local/bin/lemminx && chmod +x ~/.local/bin/lemminx"#,
                "",
            ),
            DapConfig::default(),
        ),
        manifest(
            "yaml",
            lsp(
                "yaml-language-server",
                "npm install -g yaml-language-server",
                "",
                "",
                "",
            ),
            DapConfig::default(),
        ),
    ]
}

/// Which manifest component (`lsp` or `dap`) an allow-list entry covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtComponent {
    Lsp,
    Dap,
}

impl std::fmt::Display for ExtComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ExtComponent::Lsp => "lsp",
            ExtComponent::Dap => "dap",
        })
    }
}

/// A single known-missing (manifest, component, platform) install command.
/// Every entry names the issue that will remove it — the gate ratchets: this
/// list only ever shrinks as gaps get filled in, and it is a *request-changes*
/// to delete an entry without also fixing its manifest (the whole point of
/// this test).
struct AllowedGap {
    manifest: &'static str,
    component: ExtComponent,
    platform: vimcode_core::core::extensions::Platform,
    issue: &'static str,
}

#[test]
fn registry_conformance_every_manifest_resolves_install_on_all_platforms() {
    use vimcode_core::core::extensions::Platform;

    // Confirmed RED against unfixed `develop` before this allow-list existed:
    // with `ALLOWED_GAPS` emptied out, this test fails with 27 unexpected
    // gaps (9 lsp + 18 dap) — see the PR description for the itemised list.
    //
    // #919 is this issue itself: it introduces the gate and inventories the
    // debt as of today. Splitting each row below into its own tracking issue
    // is follow-up filing work for the coordinator (a `gh issue create` step
    // this worker session cannot perform per its operating rules) — until
    // that happens every row cites #919 as the issue that will remove it.
    const ALLOWED_GAPS: &[AllowedGap] = &[
        // --- lsp: java has no install command anywhere (#918 filled in
        // macOS only in the *local test fixture*; the live registry was
        // never updated to match — see `registry_conformance_snapshot`'s
        // doc comment). ---
        AllowedGap {
            manifest: "java",
            component: ExtComponent::Lsp,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "java",
            component: ExtComponent::Lsp,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "java",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        // --- lsp: Windows has no winget/generic equivalent for these
        // brew-only / curl+mkdir-only installs yet. ---
        AllowedGap {
            manifest: "bicep",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "cpp",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "lua",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "markdown",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "terraform",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "xml",
            component: ExtComponent::Lsp,
            platform: Platform::Windows,
            issue: "#919",
        },
        // --- dap: these six adapters install via `dap_manager.rs`'s
        // hardcoded fallback table (VSIX/tarball extraction, a managed
        // venv, or "no automated install" for js-debug/java-debug), never
        // through the manifest's own `dap.install*` fields. That fallback
        // table lives outside this issue's file scope
        // (`src/core/extensions.rs`, `tests/extensions.rs`) — moving it
        // into the registry data (or otherwise making the manifest
        // self-describing) is follow-up work, not a `#919` fix-in-place.
        AllowedGap {
            manifest: "cpp",
            component: ExtComponent::Dap,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "cpp",
            component: ExtComponent::Dap,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "cpp",
            component: ExtComponent::Dap,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "csharp",
            component: ExtComponent::Dap,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "csharp",
            component: ExtComponent::Dap,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "csharp",
            component: ExtComponent::Dap,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "java",
            component: ExtComponent::Dap,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "java",
            component: ExtComponent::Dap,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "java",
            component: ExtComponent::Dap,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "javascript",
            component: ExtComponent::Dap,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "javascript",
            component: ExtComponent::Dap,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "javascript",
            component: ExtComponent::Dap,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "python",
            component: ExtComponent::Dap,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "python",
            component: ExtComponent::Dap,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "python",
            component: ExtComponent::Dap,
            platform: Platform::Windows,
            issue: "#919",
        },
        AllowedGap {
            manifest: "rust",
            component: ExtComponent::Dap,
            platform: Platform::Linux,
            issue: "#919",
        },
        AllowedGap {
            manifest: "rust",
            component: ExtComponent::Dap,
            platform: Platform::MacOS,
            issue: "#919",
        },
        AllowedGap {
            manifest: "rust",
            component: ExtComponent::Dap,
            platform: Platform::Windows,
            issue: "#919",
        },
    ];

    // A conservative deny-list of Unix-shell-only constructs that `cmd /C`
    // (the Windows invocation in `lsp_manager.rs`) cannot run. Prose over a
    // real shell parser: good enough to catch the two ways this repo's
    // manifests break (a `mkdir -p ...&&...` pipeline, or `curl ... | cut`),
    // and a real shell parser is more machinery than a deny-list of
    // substrings that have never once been valid `cmd /C` syntax.
    const WINDOWS_UNSAFE_SUBSTRINGS: &[&str] = &[
        "mkdir -p", // cmd's `mkdir` has no `-p` flag
        "| cut",    // `cut` doesn't exist on Windows
        "ln -sf",   // symlinking via `ln` doesn't exist on Windows
        "$(",       // `$(...)` command substitution is shell-only
        "chmod +x", // no execute bit / `chmod` on Windows
        "sudo ",    // no `sudo` on Windows
        "'",        // single-quote quoting is a no-op string in `cmd /C`, not string quoting
    ];

    let manifests = registry_conformance_snapshot();
    assert_eq!(
        manifests.len(),
        19,
        "snapshot should mirror the live registry's 19 extensions (#919)"
    );

    let mut unexpected_gaps: Vec<String> = Vec::new();
    let mut stale_allowlist_entries: Vec<String> = Vec::new();
    let mut windows_unsafe: Vec<String> = Vec::new();

    let find_gap = |manifest: &str, component: ExtComponent, platform: Platform| {
        ALLOWED_GAPS
            .iter()
            .find(|g| g.manifest == manifest && g.component == component && g.platform == platform)
    };

    for m in &manifests {
        // --- lsp: every manifest that actually ships an LSP server (i.e.
        // declares a binary) must resolve an install command on all three
        // platforms. Manifests with no LSP at all (git-insights: Lua
        // scripts only) are correctly exempt — there is nothing to install.
        if !m.lsp.binary.is_empty() {
            for platform in Platform::ALL {
                // #1345: a manifest declaring `[lsp.acquire]` resolves an
                // install on every platform its kind supports — all three
                // kinds (hashicorp-release/github-release/url-template) are
                // platform-neutral by construction (no `sh`), so `acquire`
                // being present at all counts as "resolved" here, same as a
                // non-empty `install_cmd_for`.
                let resolves =
                    !m.lsp.install_cmd_for(platform).is_empty() || m.lsp.acquire.is_some();
                let gap = find_gap(&m.name, ExtComponent::Lsp, platform);
                match (resolves, gap) {
                    (false, None) => unexpected_gaps.push(format!(
                        "{} (lsp) has no install command for {platform} and is not in ALLOWED_GAPS",
                        m.name
                    )),
                    (true, Some(gap)) => stale_allowlist_entries.push(format!(
                        "{} (lsp/{platform}) is allow-listed under {} but already resolves a \
                         non-empty install command — remove this ALLOWED_GAPS entry",
                        m.name, gap.issue
                    )),
                    _ => {}
                }

                if platform == Platform::Windows && resolves {
                    let cmd = m.lsp.install_cmd_for(platform);
                    for needle in WINDOWS_UNSAFE_SUBSTRINGS {
                        if cmd.contains(needle) {
                            windows_unsafe.push(format!(
                                "{} (lsp) Windows install command contains Unix-shell-only \
                                 `{needle}`, which `cmd /C` cannot run: {cmd}",
                                m.name
                            ));
                        }
                    }
                }
            }
        }

        // --- dap: every manifest that declares a dap.adapter must resolve
        // an install command on all three platforms.
        if !m.dap.adapter.is_empty() {
            for platform in Platform::ALL {
                // #1345: same "acquire resolves on every platform" rule as
                // the lsp block above.
                let resolves =
                    !m.dap.install_cmd_for(platform).is_empty() || m.dap.acquire.is_some();
                let gap = find_gap(&m.name, ExtComponent::Dap, platform);
                match (resolves, gap) {
                    (false, None) => unexpected_gaps.push(format!(
                        "{} (dap/{}) has no install command for {platform} and is not in \
                         ALLOWED_GAPS",
                        m.name, m.dap.adapter
                    )),
                    (true, Some(gap)) => stale_allowlist_entries.push(format!(
                        "{} (dap/{}/{platform}) is allow-listed under {} but already resolves a \
                         non-empty install command — remove this ALLOWED_GAPS entry",
                        m.name, m.dap.adapter, gap.issue
                    )),
                    _ => {}
                }

                if platform == Platform::Windows && resolves {
                    let cmd = m.dap.install_cmd_for(platform);
                    for needle in WINDOWS_UNSAFE_SUBSTRINGS {
                        if cmd.contains(needle) {
                            windows_unsafe.push(format!(
                                "{} (dap/{}) Windows install command contains Unix-shell-only \
                                 `{needle}`, which `cmd /C` cannot run: {cmd}",
                                m.name, m.dap.adapter
                            ));
                        }
                    }
                }
            }
        }
    }

    assert!(
        unexpected_gaps.is_empty(),
        "registry conformance gate (#919): every manifest must resolve an install command on \
         every platform unless explicitly allow-listed. Unexpected gaps (not covered by \
         ALLOWED_GAPS):\n{}",
        unexpected_gaps.join("\n")
    );
    assert!(
        stale_allowlist_entries.is_empty(),
        "registry conformance gate (#919): the allow-list only ever shrinks — these entries now \
         resolve a working install command and must be deleted:\n{}",
        stale_allowlist_entries.join("\n")
    );
    assert!(
        windows_unsafe.is_empty(),
        "registry conformance gate (#919): Windows install commands must not use Unix-shell-only \
         syntax `cmd /C` cannot run:\n{}",
        windows_unsafe.join("\n")
    );

    // Sanity check on the allow-list's own bookkeeping: every entry must
    // reference a manifest that actually exists in the snapshot, so a typo'd
    // `manifest`/`component` pair can't silently allow-list nothing (and
    // thus never get exercised by the loop above).
    for gap in ALLOWED_GAPS {
        let m = manifests
            .iter()
            .find(|m| m.name == gap.manifest)
            .unwrap_or_else(|| panic!("ALLOWED_GAPS references unknown manifest {}", gap.manifest));
        match gap.component {
            ExtComponent::Lsp => assert!(
                !m.lsp.binary.is_empty(),
                "ALLOWED_GAPS has an lsp entry for {}, which has no lsp.binary",
                gap.manifest
            ),
            ExtComponent::Dap => assert!(
                !m.dap.adapter.is_empty(),
                "ALLOWED_GAPS has a dap entry for {}, which has no dap.adapter",
                gap.manifest
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// #1345: native tool acquisition — resolver + conformance-gate integration
// ---------------------------------------------------------------------------

/// Build a throwaway zip fixture containing a single entry, for driving
/// `ext_install_from_registry`'s native-acquisition path against a
/// `file://` URL (no real network — see `tool_acquire.rs`'s own
/// `acquire_and_install_end_to_end_via_file_url` for why `curl file://` is
/// a legitimate substitute for a real HTTPS round trip in tests).
fn make_fixture_zip(
    dir: &std::path::Path,
    entry_name: &str,
    contents: &[u8],
) -> std::path::PathBuf {
    let path = dir.join("archive.zip");
    let file = std::fs::File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    writer.start_file(entry_name, options).unwrap();
    std::io::Write::write_all(&mut writer, contents).unwrap();
    writer.finish().unwrap();
    path
}

#[test]
fn ext_install_with_acquire_runs_no_terminal_command_and_registers_lsp() {
    // #1345's core acceptance: `ext_install_from_registry` on a manifest
    // with `[lsp.acquire]` and no `install_*` must (1) queue no terminal
    // command — no shell at all — and (2) once the background acquisition
    // completes, register and start the LSP server, the same outcome
    // `finalize_install_from_terminal` produces for the legacy shell path.
    //
    // Confirmed RED against unfixed `develop` before this landed: with no
    // `acquire` wiring, this manifest (no `install_linux`/`install_macos`/
    // `install_windows` either) fell through both branches — `status_parts`
    // stayed empty and the extension was marked installed with the LSP
    // server never registered, which this test's final assertions would
    // have failed to observe.
    use vimcode_core::core::extensions::{ExtensionManifest, LspConfig};
    use vimcode_core::core::tool_acquire::{AcquireConfig, AcquireKind};

    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_ext_install_acquire_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());
    // `file://` download URLs are a *test-build* allowance only (second
    // review round on #1345 — a registry manifest must not be able to turn
    // `curl` into an arbitrary-file-read). This is a separate integration
    // crate, so the library's own `cfg(test)` allowance doesn't apply here
    // and the fixture has to opt in explicitly.
    let _file_url_guard = EnvVarGuard::set(
        vimcode_core::core::tool_acquire::ALLOW_FILE_URL_DOWNLOADS_ENV,
        std::ffi::OsStr::new("1"),
    );

    let fixture_dir = std::env::temp_dir().join(format!(
        "vimcode_test_ext_install_acquire_fixture_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&fixture_dir);
    std::fs::create_dir_all(&fixture_dir).unwrap();
    let binary_name = "vimcode-test-acquire-lsp-1345";
    let archive = make_fixture_zip(&fixture_dir, binary_name, b"#!fake-lsp-server");

    let manifest = ExtensionManifest {
        name: "acquire-test-ext".to_string(),
        display_name: "Acquire Test Extension".to_string(),
        language_ids: vec!["acquiretestlang".to_string()],
        lsp: LspConfig {
            binary: binary_name.to_string(),
            acquire: Some(AcquireConfig {
                kind: AcquireKind::UrlTemplate,
                url: format!("file://{}", archive.display()),
                version: "1.0.0".to_string(),
                binary_path: binary_name.to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut e = engine_with("");
    e.ext_registry = Some(vec![manifest]);

    let action = exec(&mut e, "ExtInstall acquire-test-ext");
    assert_eq!(
        action,
        vimcode_core::EngineAction::None,
        "native acquisition must not open a terminal pane (no RunInTerminal action)"
    );
    assert!(
        e.pending_terminal_command.is_none(),
        "native acquisition must queue no shell command"
    );
    assert!(
        e.message.to_lowercase().contains("acquiring"),
        "status message should say acquisition is underway: {}",
        e.message
    );

    // Drain the background acquisition thread (poll until the channel
    // yields — bounded so a genuine regression fails fast instead of
    // hanging the suite).
    let mut drained = false;
    for _ in 0..200 {
        if e.poll_tool_acquire() {
            drained = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(drained, "background acquisition did not complete in time");

    assert!(
        e.message.contains("installed and started"),
        "finalize message should report the server started: {}",
        e.message
    );
    assert!(
        vimcode_core::core::lsp_manager::resolve_command(binary_name).is_some(),
        "the acquired binary should now be resolvable via the managed tools dir"
    );

    let _ = std::fs::remove_dir_all(&data_home);
    let _ = std::fs::remove_dir_all(&fixture_dir);
}

#[test]
fn acquire_rejects_file_url_without_the_test_only_override() {
    // Second review round on #1345: `validate_download_url` used to accept
    // `file://` unconditionally, in production code. A community-submitted
    // or compromised registry manifest could therefore declare
    // `[lsp.acquire] url = "file:///home/user/.ssh/id_rsa"` and have vimcode
    // `curl` an arbitrary readable file into a predictable staging path.
    //
    // This runs in a separate integration crate, so the library's
    // `cfg(test)` fixture allowance is *off* here — exactly the
    // configuration a shipped binary is in, modulo the `debug_assertions`
    // env-var opt-in which this test explicitly clears.
    //
    // RED against the previous commit: `acquire_and_install_for` there
    // downloaded the file and got as far as the archive-format check.
    use vimcode_core::core::extensions::Platform;
    use vimcode_core::core::tool_acquire::{
        acquire_and_install_for, AcquireConfig, AcquireError, AcquireKind, Arch,
        ALLOW_FILE_URL_DOWNLOADS_ENV,
    };

    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_acquire_file_url_denied_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());
    let _no_override = EnvVarGuard::unset(ALLOW_FILE_URL_DOWNLOADS_ENV);

    // A real, readable file — so a pass can only mean the scheme was
    // refused, not that the path happened not to exist.
    let secret = data_home.join("pretend-private-key");
    std::fs::write(&secret, b"-----BEGIN OPENSSH PRIVATE KEY-----\n").unwrap();

    let cfg = AcquireConfig {
        kind: AcquireKind::UrlTemplate,
        url: format!("file://{}", secret.display()),
        version: "1.0.0".to_string(),
        binary_path: "vimcode-test-file-url-tool".to_string(),
        ..Default::default()
    };

    let err = acquire_and_install_for(
        "vimcode-test-file-url-tool",
        &cfg,
        Platform::Linux,
        Arch::Amd64,
    )
    .expect_err("a file:// acquire URL must be refused outside test builds");
    assert!(
        matches!(err, AcquireError::BadConfig(_)),
        "expected the https-only scheme check to reject it, got {err:?}"
    );
    assert!(
        !vimcode_core::core::paths::managed_tool_dir("vimcode-test-file-url-tool").exists(),
        "nothing should have been staged or installed for a refused URL"
    );

    let _ = std::fs::remove_dir_all(&data_home);
}

#[test]
fn registry_conformance_treats_acquire_as_resolving_every_platform() {
    // #1345's acceptance criterion, isolated from the big registry snapshot
    // above (no live manifest declares `acquire` yet — that's the follow-up
    // registry-side adoption, not this issue): a manifest with `[lsp.acquire]`
    // and no `install_linux`/`install_macos`/`install_windows` resolves an
    // install on every platform.
    use vimcode_core::core::extensions::{ExtensionManifest, LspConfig, Platform};
    use vimcode_core::core::tool_acquire::{AcquireConfig, AcquireKind};

    let manifest = ExtensionManifest {
        name: "terraform".to_string(),
        display_name: "Terraform".to_string(),
        lsp: LspConfig {
            binary: "terraform-ls".to_string(),
            acquire: Some(AcquireConfig {
                kind: AcquireKind::HashicorpRelease,
                product: "terraform-ls".to_string(),
                binary_path: "terraform-ls".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    // No install_* fields are set at all — the old resolver would report a
    // gap on every platform. Confirmed RED against unfixed `develop` before
    // this landed: `install_cmd_for(platform).is_empty()` was `true` on all
    // three platforms and nothing accounted for `acquire`.
    for platform in Platform::ALL {
        assert!(
            manifest.lsp.install_cmd_for(platform).is_empty(),
            "sanity: this manifest has no install_* command"
        );
        let resolves =
            !manifest.lsp.install_cmd_for(platform).is_empty() || manifest.lsp.acquire.is_some();
        assert!(
            resolves,
            "acquire should count as resolving an install on {platform}"
        );
    }
}

/// `VIMCODE_TEST_DATA_HOME`/`PATH` are process-global env state; serialize
/// with a dedicated lock — mirrors `HOMEBREW_ENV_LOCK` above (#917) but kept
/// separate since these tests touch different variables and shouldn't
/// contend with the Homebrew ones.
static TOOL_ACQUIRE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn resolve_command_finds_managed_tool_binary_with_empty_path() {
    // #1345 acceptance: "a resolver test: a binary under the managed dir is
    // found with an empty PATH." Simulates a vimcode-managed acquisition by
    // writing directly into a throwaway `VIMCODE_TEST_DATA_HOME`, without
    // going through the network-touching `tool_acquire::acquire_and_install`.
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_managed_resolver_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();

    let binary_name = "vimcode-test-managed-terraform-ls-1345";
    let version_dir = data_home.join("tools").join(binary_name).join("0.32.0");
    std::fs::create_dir_all(&version_dir).unwrap();
    let binary_path = version_dir.join(binary_name);
    std::fs::write(&binary_path, b"#!/bin/sh\necho fake\n").unwrap();
    std::fs::write(
        data_home.join("tools").join(binary_name).join("current"),
        "0.32.0",
    )
    .unwrap();

    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());
    // Empty PATH (well, just enough to run `which` itself if the resolver's
    // PATH fallback were reached) — proves resolution happens via the
    // managed dir, not by falling through to a PATH lookup that happens to
    // find something with the same name.
    let _path_guard = EnvVarGuard::set("PATH", std::ffi::OsStr::new(""));

    let resolved = vimcode_core::core::lsp_manager::resolve_command(binary_name);
    assert_eq!(
        resolved,
        Some(binary_path.clone()),
        "should resolve {binary_name} via the managed tools dir with PATH empty"
    );

    let _ = std::fs::remove_dir_all(&data_home);
}

#[test]
fn dap_resolve_binary_finds_managed_tool_binary_with_empty_path() {
    // Second review round on #1345: `[dap.acquire]` unpacks the adapter into
    // the vimcode-managed tools dir and paints "DAP adapter for '…'
    // installed — press F5 to debug", but `DapManager::start_adapter`
    // resolved the adapter through `dap_manager::resolve_binary`, which only
    // ever looked at Mason's bin dir and `PATH` — neither of which contains a
    // vimcode-managed tool. F5 then failed with "DAP binary '…' not found"
    // for an adapter vimcode had *just* installed: the same "install check
    // and launch use two different lookups" split #1344 closed on the LSP
    // side. `resolve_binary` now delegates to the shared
    // `lsp_manager::resolve_command`.
    //
    // RED against the previous commit: `resolve_binary` returned `None` here
    // (empty `PATH`, no Mason dir), so this assertion failed.
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_managed_dap_resolver_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();

    let binary_name = "vimcode-test-managed-codelldb-1345";
    let version_dir = data_home.join("tools").join(binary_name).join("1.10.0");
    std::fs::create_dir_all(&version_dir).unwrap();
    let binary_path = version_dir.join(binary_name);
    std::fs::write(&binary_path, b"#!/bin/sh\necho fake-adapter\n").unwrap();
    std::fs::write(
        data_home.join("tools").join(binary_name).join("current"),
        "1.10.0",
    )
    .unwrap();

    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());
    let _path_guard = EnvVarGuard::set("PATH", std::ffi::OsStr::new(""));

    assert_eq!(
        vimcode_core::core::dap_manager::resolve_binary(binary_name),
        Some(binary_path.clone()),
        "the DAP launch path must resolve a vimcode-acquired adapter via the managed tools dir"
    );
    // …and it agrees with the LSP-side resolver, which is the whole point of
    // routing both through one lookup.
    assert_eq!(
        vimcode_core::core::dap_manager::resolve_binary(binary_name),
        vimcode_core::core::lsp_manager::resolve_command(binary_name),
        "install-time check and debug-start launch must resolve identically"
    );

    let _ = std::fs::remove_dir_all(&data_home);
}

#[test]
fn resolve_command_falls_through_when_no_managed_tool_current_pointer() {
    // A tool directory can exist with old/removed versions but no `current`
    // pointer (e.g. mid-acquisition, or after a manual cleanup) — resolution
    // must not find a binary in that case, so this doesn't accidentally
    // resolve to a stale/uninstalled version.
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_managed_no_current_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();

    let binary_name = "vimcode-test-managed-no-current-1345";
    let version_dir = data_home.join("tools").join(binary_name).join("0.1.0");
    std::fs::create_dir_all(&version_dir).unwrap();
    std::fs::write(version_dir.join(binary_name), b"stale").unwrap();
    // Deliberately no `current` pointer file written.

    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());
    let _path_guard = EnvVarGuard::set("PATH", std::ffi::OsStr::new(""));

    let resolved = vimcode_core::core::lsp_manager::resolve_command(binary_name);
    assert_eq!(resolved, None);

    let _ = std::fs::remove_dir_all(&data_home);
}

/// Live, network-touching operator smoke test (#1345 acceptance: "one
/// `#[ignore]`d live test that acquires terraform-ls on the host platform").
/// Not run by `cargo test` — run explicitly with
/// `cargo test --test extensions -- --ignored acquire_terraform_ls_live_smoke`.
#[test]
#[ignore]
fn acquire_terraform_ls_live_smoke() {
    use vimcode_core::core::extensions::Platform;
    use vimcode_core::core::tool_acquire::{
        acquire_and_install_for, AcquireConfig, AcquireKind, Arch,
    };

    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let data_home =
        std::env::temp_dir().join(format!("vimcode_test_live_acquire_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_home);
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());

    let cfg = AcquireConfig {
        kind: AcquireKind::HashicorpRelease,
        product: "terraform-ls".to_string(),
        binary_path: "terraform-ls".to_string(),
        ..Default::default()
    };

    let installed = acquire_and_install_for("terraform-ls", &cfg, Platform::host(), Arch::host())
        .expect("live acquisition of terraform-ls should succeed against the real HashiCorp API");
    assert!(installed.is_file(), "{} should exist", installed.display());

    let resolved = vimcode_core::core::lsp_manager::resolve_command("terraform-ls");
    assert_eq!(
        resolved,
        Some(installed),
        "resolve_command should find the just-acquired terraform-ls via the managed dir"
    );

    let _ = std::fs::remove_dir_all(&data_home);
}

// ═══════════════════════════════════════════════════════════════════════════
// #1214 — the live engine seam: immediate `vimcode.buffer.*` /
// `vimcode.window.*`, handles, reentrancy, undo grouping.
//
// The legacy `vimcode.buf.*` surface is snapshot-in / queue-out: reads come
// from a rope clone taken before the callback ran, writes replay afterwards.
// The tests below pin the *immediate* surface (reads and writes go straight
// through a live `&mut Engine`) and, alongside it, pin that the legacy
// semantics did not move.
// ═══════════════════════════════════════════════════════════════════════════

/// The kill test, native form: a `set_lines` followed by a `get_lines` in the
/// *same* callback must read back what was just written — and without the
/// trailing newline ropey's `Rope::line()` would carry.
///
/// RED against unfixed `develop`: there is no `vimcode.buffer` table at all
/// there, so the callback errors out and `message` stays empty.
#[test]
fn immediate_set_lines_then_get_lines_reads_back_the_write() {
    let mut e = engine_with_plugin(
        "alpha\nbeta\ngamma\n",
        "live_read_after_write",
        r#"
        vimcode.command("LiveRoundTrip", function(_)
            vimcode.buffer.set_lines(0, 0, 1, { "REWRITTEN" })
            local back = vimcode.buffer.get_lines(0, 0, 1)
            vimcode.message("read=[" .. back[1] .. "] len=" .. #back[1])
        end)
        "#,
    );
    exec(&mut e, "LiveRoundTrip");
    assert_eq!(
        e.message, "read=[REWRITTEN] len=9",
        "get_lines must observe the immediate write and strip the terminator"
    );
    assert_eq!(
        buf(&e),
        "REWRITTEN\nbeta\ngamma\n",
        "the immediate write must have landed in the buffer"
    );
}

/// Negative indices count from the end and `line_count` is the *logical* line
/// count (no phantom trailing empty line), so a full-buffer read/write
/// round-trips.
#[test]
fn immediate_line_count_and_negative_indices_round_trip() {
    let mut e = engine_with_plugin(
        "one\ntwo\nthree\n",
        "live_indices",
        r#"
        vimcode.command("LiveIdx", function(_)
            local n = vimcode.buffer.line_count(0)
            local last = vimcode.buffer.get_lines(0, -1, n)
            local all = vimcode.buffer.get_lines(0, 0, n)
            vimcode.buffer.set_lines(0, 0, n, all)
            vimcode.message("n=" .. n .. " last=" .. last[1] .. " all=" .. #all)
        end)
        "#,
    );
    exec(&mut e, "LiveIdx");
    assert_eq!(
        e.message, "n=3 last=three all=3",
        "line_count must exclude ropey's phantom trailing line"
    );
    assert_eq!(
        buf(&e),
        "one\ntwo\nthree\n",
        "writing back everything get_lines returned must be a no-op"
    );
}

/// `vimcode.buffer.create{scratch=true}` returns a handle that can be written,
/// read back, and shown with `vimcode.window.set_buf` — all inside one
/// callback, on a buffer the plugin did not open.
#[test]
fn immediate_created_scratch_buffer_can_be_written_read_and_shown() {
    let mut e = engine_with_plugin(
        "original\n",
        "live_scratch",
        r#"
        vimcode.command("LiveScratch", function(_)
            local b = vimcode.buffer.create({ scratch = true, name = "ZQSCRATCH" })
            vimcode.buffer.set_lines(b, 0, -1, { "scratch one", "scratch two" })
            local back = vimcode.buffer.get_lines(b, 0, 2)
            local shown = vimcode.window.set_buf(0, b)
            vimcode.message(
                "b=" .. b
                .. " valid=" .. tostring(vimcode.buffer.is_valid(b))
                .. " back=" .. back[2]
                .. " shown=" .. tostring(shown)
                .. " cur=" .. vimcode.buffer.current()
            )
        end)
        "#,
    );
    exec(&mut e, "LiveScratch");
    let msg = e.message.clone();
    assert!(
        msg.contains("valid=true")
            && msg.contains("back=scratch two")
            && msg.contains("shown=true"),
        "scratch buffer must be writable, readable and showable: {msg}"
    );
    // `current()` is read *before* the window switch is visible to Lua? No —
    // set_buf is immediate, so the current buffer is already the new one.
    let handle = e.active_buffer_id().0;
    assert!(
        msg.contains(&format!("b={handle} ")) && msg.contains(&format!("cur={handle}")),
        "window.set_buf must have made the created buffer current: {msg}"
    );
    assert_eq!(
        buf(&e),
        "scratch one\nscratch two\n",
        "the window must now show the scratch buffer's immediate content"
    );
}

/// A Lua function stashed in a table at plugin-load time and invoked from a
/// *later* event can use the immediate API. This is the constraint that decided
/// the seam's design (a `Lua::scope`-based mechanism cannot express it), and
/// the shape a timer or UI event handler will have.
#[test]
fn stored_lua_callback_invoked_from_a_later_event_uses_the_immediate_api() {
    let mut e = engine_with_plugin(
        "before\n",
        "live_stored_cb",
        r#"
        local M = {}
        M.handlers = {}
        M.handlers.on_insert = function()
            vimcode.buffer.set_lines(0, 0, 1, { "written by stored callback" })
            local back = vimcode.buffer.get_lines(0, 0, 1)
            vimcode.message("stored=" .. back[1])
        end
        vimcode.on("InsertEnter", function(_) M.handlers.on_insert() end)
        "#,
    );
    assert_eq!(e.message, "", "precondition: nothing has run yet");
    // A genuinely later event, fired through production code.
    e.set_mode(vimcode_core::Mode::Insert);
    assert_eq!(
        e.message, "stored=written by stored callback",
        "a callback stored at load time must reach the live engine when \
         invoked from a later event"
    );
    assert_eq!(buf(&e), "written by stored callback\n");
}

/// A plugin-triggered buffer switch fires `BufEnter` *from inside* the plugin
/// callback. Pre-#1214 that nested event silently vanished (`plugin_event`
/// `take()`-ed the manager for the duration of the call, so the nested
/// dispatch found `None`); now it is deferred and dispatched as soon as the
/// outer callback returns.
#[test]
fn plugin_triggered_buffer_switch_fires_a_nested_event() {
    let mut e = engine_with_plugin(
        "original\n",
        "live_nested_event",
        r#"
        vimcode.on("BufEnter", function(name)
            local n = vimcode.buffer.line_count(0)
            vimcode.buffer.set_lines(0, n, n, { "BufEnter saw " .. name })
        end)
        vimcode.command("LiveSwitch", function(_)
            local b = vimcode.buffer.create({ scratch = true, name = "ZQNEST" })
            vimcode.buffer.set_lines(b, 0, -1, { "created" })
            vimcode.window.set_buf(0, b)
        end)
        "#,
    );
    exec(&mut e, "LiveSwitch");
    assert_eq!(
        buf(&e),
        "created\nBufEnter saw [ZQNEST]\n",
        "the nested BufEnter must actually run (deferred, after the outer \
         callback), not silently no-op"
    );
}

/// Undo grouping must not regress: the queued path replays a plugin's edits as
/// one batch, and the immediate path must behave the same way — ten immediate
/// writes leave one undo step, and one `u` restores the original text.
#[test]
fn immediate_multi_line_plugin_edit_is_a_single_undo_step() {
    let mut e = engine_with_plugin(
        "L0\nL1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\n",
        "live_undo",
        r#"
        vimcode.command("LiveEditAll", function(_)
            for i = 0, 9 do
                vimcode.buffer.set_lines(0, i, i + 1, { "E" .. i })
            end
        end)
        "#,
    );
    let before = buf(&e);
    // Force the undo tree's root node to exist before measuring, so the count
    // below reflects only what the plugin committed (the first
    // `start_undo_group` on a buffer seeds the root, which also bumps the
    // commit counter).
    e.start_undo_group();
    e.finish_undo_group();
    let commits_before = e.active_buffer_state().undo_commit_count();
    exec(&mut e, "LiveEditAll");
    assert_eq!(buf(&e), "E0\nE1\nE2\nE3\nE4\nE5\nE6\nE7\nE8\nE9\n");
    let committed = e.active_buffer_state().undo_commit_count() - commits_before;
    assert_eq!(
        committed, 1,
        "ten immediate writes in one callback must commit exactly one undo \
         step, not ten"
    );
    e.undo();
    assert_eq!(
        buf(&e),
        before,
        "a single undo must restore the whole plugin edit"
    );
}

/// Scope item 4: the legacy queued surface keeps its old semantics exactly —
/// reads inside the callback are stale (they come from the pre-call snapshot)
/// *and* they still carry ropey's trailing newline. Extensions in
/// `JDonaghy/vimcode-ext` depend on both.
#[test]
fn legacy_buf_api_stays_stale_and_keeps_its_terminator() {
    let mut e = engine_with_plugin(
        "alpha\nbeta\n",
        "legacy_unchanged",
        r#"
        vimcode.command("LegacyRoundTrip", function(_)
            vimcode.buf.set_lines(0, 1, { "LEGACY" })
            local back = vimcode.buf.get_lines(0, 1)
            vimcode.message("read=[" .. back[1] .. "] len=" .. #back[1])
        end)
        "#,
    );
    exec(&mut e, "LegacyRoundTrip");
    assert_eq!(
        e.message, "read=[alpha\n] len=6",
        "the legacy read must stay stale and keep its trailing newline"
    );
    assert_eq!(
        buf(&e),
        "LEGACY\nbeta\n",
        "the legacy write must still be replayed after the callback returns"
    );
}

/// Mixing the two tiers in one callback: the queued write is applied *after*
/// the callback returns, so it lands last and wins — documented order, pinned.
#[test]
fn queued_write_is_applied_after_an_immediate_write_to_the_same_line() {
    let mut e = engine_with_plugin(
        "alpha\n",
        "live_mixed_order",
        r#"
        vimcode.command("LiveMixed", function(_)
            vimcode.buf.set_lines(0, 1, { "from queued API" })
            vimcode.buffer.set_lines(0, 0, 1, { "from immediate API" })
            local back = vimcode.buffer.get_lines(0, 0, 1)
            vimcode.message("during=" .. back[1])
        end)
        "#,
    );
    exec(&mut e, "LiveMixed");
    assert_eq!(
        e.message, "during=from immediate API",
        "during the callback the immediate write is what is visible"
    );
    assert_eq!(
        buf(&e),
        "from queued API\n",
        "the queued write replays afterwards and therefore wins"
    );
}

/// Window handles: `0` means "current", cursor reads/writes are 1-indexed and
/// clamped, and both the named and positional table shapes are accepted.
#[test]
fn immediate_window_cursor_is_one_indexed_and_clamped() {
    let mut e = engine_with_plugin(
        "one\ntwo\nthree\n",
        "live_window_cursor",
        r#"
        vimcode.command("LiveCursor", function(_)
            local w = vimcode.window.current()
            vimcode.window.set_cursor(w, { line = 2, col = 2 })
            local c = vimcode.window.get_cursor(0)
            vimcode.window.set_cursor(0, { 3, 99 })
            local d = vimcode.window.get_cursor(w)
            vimcode.message(
                "w=" .. w
                .. " named=" .. c.line .. "," .. c.col
                .. " pos=" .. d[1] .. "," .. d[2]
                .. " buf=" .. vimcode.window.get_buf(0)
            )
        end)
        "#,
    );
    exec(&mut e, "LiveCursor");
    let expect = format!(
        "w={} named=2,2 pos=3,5 buf={}",
        e.active_window_id().0,
        e.active_buffer_id().0
    );
    assert_eq!(
        e.message, expect,
        "cursor must be 1-indexed, clamped to the line, and readable in both \
         table shapes"
    );
    assert_eq!(e.cursor().line, 2, "engine cursor line (0-indexed)");
    assert_eq!(e.cursor().col, 4, "engine cursor col clamped to 'three'");
}

/// The immediate API is only valid while a callback is running. Called at
/// plugin *load* time there is no live engine, and the call must raise a clear
/// Lua error rather than silently doing nothing (or worse).
#[test]
fn immediate_api_outside_a_callback_is_a_clear_lua_error() {
    let dir = plugin_fixture_dir("live_no_engine");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("noengine.lua"),
        "vimcode.buffer.set_lines(0, 0, 1, { \"nope\" })\n",
    )
    .unwrap();
    let mut mgr = vimcode_core::core::plugin::PluginManager::new().unwrap();
    mgr.load_plugins_dir(&dir, &[]);
    let err = mgr.plugins[0]
        .error
        .clone()
        .expect("calling the immediate API at load time must be an error");
    assert!(
        err.contains("no live editor"),
        "the error must say the immediate API needs a running callback: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Native Extension API — Phase 2 (#1623): new events, keymaps consulted
// before built-ins (expr / buffer-local / o / x), g@ marks, keymap
// enumeration, and a BufWriteCmd-style write hook.
// ═══════════════════════════════════════════════════════════════════════════

// ── New events ──────────────────────────────────────────────────────────────

#[test]
fn text_changed_fires_on_normal_mode_edit_with_buffer_handle() {
    let mut e = engine_with_plugin(
        "hello\n",
        "text_changed",
        r#"
        vimcode.on("TextChanged", function(buf)
            vimcode.message("TextChanged:" .. buf)
        end)
        "#,
    );
    let buf = e.active_buffer_id().0;
    // 'x' deletes a char in Normal mode — a real buffer edit outside Insert.
    press(&mut e, 'x');
    assert_eq!(
        e.message,
        format!("TextChanged:{buf}"),
        "TextChanged must fire once, delivering the buffer handle"
    );
}

#[test]
fn text_changed_does_not_fire_for_a_non_editing_key() {
    let mut e = engine_with_plugin(
        "hello\n",
        "text_changed_quiet",
        r#"
        vimcode.on("TextChanged", function(buf)
            vimcode.message("TextChanged:" .. buf)
        end)
        "#,
    );
    // 'l' just moves the cursor — no buffer mutation.
    press(&mut e, 'l');
    assert_eq!(
        e.message, "",
        "TextChanged must not fire for a key that didn't change the buffer"
    );
}

#[test]
fn text_changed_i_fires_while_typing_in_insert_mode() {
    let mut e = engine_with_plugin(
        "hello\n",
        "text_changed_i",
        r#"
        vimcode.on("TextChangedI", function(buf)
            vimcode.message("TextChangedI:" .. buf)
        end)
        vimcode.on("TextChanged", function(buf)
            vimcode.message("TextChanged:" .. buf)
        end)
        "#,
    );
    let buf = e.active_buffer_id().0;
    press(&mut e, 'i');
    press(&mut e, 'X');
    assert_eq!(
        e.message,
        format!("TextChangedI:{buf}"),
        "typing in Insert mode must fire TextChangedI, not TextChanged"
    );
}

#[test]
fn cursor_moved_fires_with_window_handle_after_flush() {
    let mut e = engine_with_plugin(
        "one\ntwo\nthree\n",
        "cursor_moved",
        r#"
        vimcode.on("CursorMoved", function(win)
            vimcode.message("CursorMoved:" .. win)
        end)
        "#,
    );
    let win = e.active_window_id().0;
    press(&mut e, 'j'); // move down a line
    assert!(
        e.cursor_moved_event_pending.is_some(),
        "moving the cursor must arm the CursorMoved debounce"
    );
    // Force the debounce to have elapsed instead of sleeping in a test.
    e.cursor_moved_event_pending =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
    let redrew = e.flush_cursor_moved_event();
    assert!(redrew, "flush must report the event fired");
    assert_eq!(
        e.message,
        format!("CursorMoved:{win}"),
        "CursorMoved must deliver the window handle"
    );
}

#[test]
fn cursor_moved_i_fires_while_typing() {
    let mut e = engine_with_plugin(
        "hello\n",
        "cursor_moved_i",
        r#"
        vimcode.on("CursorMovedI", function(win)
            vimcode.message("CursorMovedI:" .. win)
        end)
        "#,
    );
    let win = e.active_window_id().0;
    press(&mut e, 'i');
    press(&mut e, 'X'); // typing moves the cursor forward
    e.cursor_moved_event_pending =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(200));
    e.flush_cursor_moved_event();
    assert_eq!(
        e.message,
        format!("CursorMovedI:{win}"),
        "cursor movement while typing must fire CursorMovedI, not CursorMoved"
    );
}

#[test]
fn buf_leave_fires_with_old_buffer_handle_on_window_set_buf() {
    let mut e = engine_with_plugin(
        "hello\n",
        "buf_leave",
        r#"
        vimcode.on("BufLeave", function(buf)
            vimcode.message("BufLeave:" .. buf)
        end)
        vimcode.command("SwitchAway", function(_)
            local b = vimcode.buffer.create({scratch = true})
            vimcode.window.set_buf(0, b)
        end)
        "#,
    );
    let old_buf = e.active_buffer_id().0;
    exec(&mut e, "SwitchAway");
    assert_eq!(
        e.message,
        format!("BufLeave:{old_buf}"),
        "BufLeave must fire for the window's previous buffer"
    );
    assert_ne!(
        e.active_buffer_id().0,
        old_buf,
        "the window must actually have switched buffers"
    );
}

#[test]
fn buf_write_pre_fires_before_the_file_is_actually_written() {
    // Per-pid, for the same reason `plugin_fixture_dir` is: a concurrent test
    // run sharing this path would have already replaced "old\n" with "new\n"
    // before `BufWritePre` reads it.
    let tmp = std::env::temp_dir().join(format!(
        "vc_plugin_bufwritepre_test_{}.txt",
        std::process::id()
    ));
    std::fs::write(&tmp, "old\n").ok();

    let mut e = engine_with_plugin(
        "",
        "buf_write_pre",
        r#"
        vimcode.on("BufWritePre", function(path)
            local f = io.open(path, "r")
            PRE_CONTENT = f and f:read("*a") or "MISSING"
            if f then f:close() end
        end)
        vimcode.command("GetPre", function(_)
            vimcode.message("pre=" .. (PRE_CONTENT or "nil"))
        end)
        "#,
    );
    e.open_file_in_tab(&tmp);
    set_content(&mut e, "new\n");
    assert!(e.save().is_ok(), "save must still succeed");
    exec(&mut e, "GetPre");
    assert_eq!(
        e.message, "pre=old\n",
        "BufWritePre must see the file's content before this write landed"
    );
    let on_disk = std::fs::read_to_string(&tmp).unwrap();
    assert_eq!(
        on_disk, "new\n",
        "the write itself must still have happened"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn win_enter_and_win_leave_fire_on_window_focus_change() {
    let mut e = engine_with_plugin(
        "hello\n",
        "win_focus",
        r#"
        vimcode.on("WinEnter", function(win)
            vimcode.message("enter:" .. win)
        end)
        vimcode.on("WinLeave", function(win)
            vimcode.message("leave:" .. win)
        end)
        "#,
    );
    let win_a = e.active_window_id().0;
    e.split_window(vimcode_core::core::window::SplitDirection::Horizontal, None);
    let win_b = e.active_window_id().0;
    assert_ne!(win_a, win_b, "split must create and focus a new window");
    // The split itself doesn't go through the focus-change helper (it's not
    // "switching focus", it's "creating and seeding a new window") — focus
    // the other window explicitly to exercise WinEnter/WinLeave.
    e.focus_prev_window();
    // Both fire (WinLeave for win_b, then WinEnter for win_a) — WinEnter
    // fires last, so it's the one still in `message`. The counted variant
    // below confirms WinLeave fired too.
    assert_eq!(
        e.message,
        format!("enter:{win_a}"),
        "WinEnter must fire for the window gaining focus"
    );
}

#[test]
fn win_enter_and_win_leave_both_fire_exactly_once() {
    let mut e = engine_with_plugin(
        "hello\n",
        "win_focus_counted",
        r#"
        ENTER_COUNT = 0
        LEAVE_COUNT = 0
        vimcode.on("WinEnter", function(_) ENTER_COUNT = ENTER_COUNT + 1 end)
        vimcode.on("WinLeave", function(_) LEAVE_COUNT = LEAVE_COUNT + 1 end)
        vimcode.command("Counts", function(_)
            vimcode.message("enter=" .. ENTER_COUNT .. " leave=" .. LEAVE_COUNT)
        end)
        "#,
    );
    e.split_window(vimcode_core::core::window::SplitDirection::Horizontal, None);
    e.focus_prev_window();
    exec(&mut e, "Counts");
    assert_eq!(
        e.message, "enter=1 leave=1",
        "each focus change must fire WinEnter/WinLeave exactly once"
    );
}

#[test]
fn file_type_fires_with_the_detected_language_on_open() {
    let tmp = std::env::temp_dir().join("vc_plugin_filetype_test.rs");
    std::fs::write(&tmp, "fn main() {}\n").ok();

    let mut e = engine_with_plugin(
        "",
        "file_type",
        r#"
        vimcode.on("FileType", function(ft)
            SEEN_FILETYPE = ft
        end)
        vimcode.command("GetFiletype", function(_)
            vimcode.message("filetype:" .. (SEEN_FILETYPE or "nil"))
        end)
        "#,
    );
    // `open_file_in_tab` fires FileType but also kicks off LSP bookkeeping
    // that overwrites `e.message` afterwards (e.g. "no LSP server found") —
    // capture the event's arg into a Lua global instead of reading `message`
    // straight after, same as the `BufWritePre` test above.
    e.open_file_in_tab(&tmp);
    exec(&mut e, "GetFiletype");
    assert_eq!(
        e.message, "filetype:rust",
        "FileType must fire with the detected language id"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn colorscheme_fires_with_the_new_scheme_name() {
    let mut e = engine_with_plugin(
        "hello\n",
        "colorscheme_event",
        r#"
        vimcode.on("ColorScheme", function(name)
            vimcode.message("scheme:" .. name)
        end)
        "#,
    );
    exec(&mut e, "colorscheme gruvbox-dark");
    assert_eq!(
        e.message, "scheme:gruvbox-dark",
        "ColorScheme must fire with the new scheme's canonical name"
    );
}

#[test]
fn user_event_fires_via_fire_event_with_the_given_name() {
    let mut e = engine_with_plugin(
        "hello\n",
        "user_event",
        r#"
        vimcode.on("User", function(name)
            vimcode.message("user:" .. name)
        end)
        vimcode.command("Fire", function(_)
            vimcode.fire_event("MyPluginReady")
        end)
        "#,
    );
    exec(&mut e, "Fire");
    assert_eq!(
        e.message, "user:MyPluginReady",
        "vimcode.fire_event must deliver the caller-chosen name via the User event"
    );
}

#[test]
fn user_event_does_not_fire_for_an_unrelated_trigger() {
    let mut e = engine_with_plugin(
        "hello\n",
        "user_event_quiet",
        r#"
        vimcode.on("User", function(name)
            vimcode.message("user:" .. name)
        end)
        "#,
    );
    press(&mut e, 'x'); // an ordinary edit, no fire_event call anywhere
    assert_eq!(
        e.message, "",
        "User must never fire unless a plugin explicitly calls vimcode.fire_event"
    );
}

// ── Keymaps consulted before built-ins (#1623) ─────────────────────────────

#[test]
fn lua_keymap_set_beats_the_built_in_substitute_s() {
    let mut e = engine_with_plugin(
        "hello\n",
        "keymap_precedence",
        r#"
        vimcode.keymap.set("n", "s", function()
            vimcode.message("plugin owns s")
        end)
        "#,
    );
    press(&mut e, 's');
    assert_eq!(
        e.message, "plugin owns s",
        "a vimcode.keymap.set map must be consulted before the built-in 's' \
         (substitute) handler"
    );
    assert_eq!(
        e.mode,
        vimcode_core::Mode::Normal,
        "the built-in 's' (which enters Insert mode) must never have run"
    );
    assert_eq!(
        buf(&e),
        "hello\n",
        "the built-in 's' must never have deleted a char"
    );
}

#[test]
fn lua_keymap_survives_rebuild_user_keymaps() {
    // `rebuild_user_keymaps` re-derives `user_keymaps` from
    // `settings.keymaps` alone (`:nnoremap`, `:unmap`, saving the Keymaps
    // editor buffer all call it) — a Lua `vimcode.keymap.set` entry isn't
    // sourced from settings at all, so a naive rebuild would silently drop
    // it. This is a regression test for that: the map must still fire after
    // a rebuild triggered for an unrelated reason.
    let mut e = engine_with_plugin(
        "hello\n",
        "keymap_survives_rebuild",
        r#"
        vimcode.keymap.set("n", "s", function()
            vimcode.message("plugin owns s")
        end)
        "#,
    );
    // Something else entirely (e.g. `:nnoremap`) triggers a rebuild.
    exec(&mut e, "nnoremap gx :join");
    press(&mut e, 's');
    assert_eq!(
        e.message, "plugin owns s",
        "a Lua keymap must survive a rebuild triggered by unrelated config \
         keymap changes"
    );
}

#[test]
fn lua_keymap_set_multi_key_sequence_beats_built_in_prefix() {
    // nvim-surround's actual shape: a plugin owns "ys" while 'y' alone keeps
    // its built-in (yank operator) meaning.
    let mut e = engine_with_plugin(
        "hello world\n",
        "keymap_ys",
        r#"
        vimcode.keymap.set("n", "ys", function()
            vimcode.message("ys fired")
        end)
        "#,
    );
    press(&mut e, 'y');
    press(&mut e, 's');
    assert_eq!(e.message, "ys fired", "the two-key Lua mapping must fire");
}

#[test]
fn lua_keymap_expr_map_feeds_returned_keys() {
    let mut e = engine_with_plugin(
        "line one\nline two\n",
        "keymap_expr",
        r#"
        vimcode.keymap.set("n", "Q", function()
            return "dd"
        end, {expr = true})
        "#,
    );
    press(&mut e, 'Q');
    assert_eq!(
        buf(&e),
        "line two\n",
        "an expr map's returned key notation must be fed back through the \
         normal key path"
    );
}

#[test]
fn lua_keymap_buffer_local_only_matches_its_own_buffer() {
    let mut e = engine_with_plugin(
        "hello\n",
        "keymap_buffer_local",
        r#"
        vimcode.command("Setup", function(_)
            local a = vimcode.buffer.current()
            vimcode.keymap.set("n", "Q", function()
                vimcode.message("Q on A")
            end, {buffer = a})
            BUF_A = a
            local b = vimcode.buffer.create({scratch = true})
            vimcode.window.set_buf(0, b)
        end)
        vimcode.command("SwitchBack", function(_)
            vimcode.window.set_buf(0, BUF_A)
        end)
        "#,
    );
    exec(&mut e, "Setup");
    // Now on the scratch buffer B — the map is bound to A only.
    press(&mut e, 'Q');
    assert_eq!(
        e.message, "",
        "a buffer-local map must not fire while a different buffer is active"
    );
    exec(&mut e, "SwitchBack");
    press(&mut e, 'Q');
    assert_eq!(
        e.message, "Q on A",
        "a buffer-local map must fire once its buffer is active again"
    );
}

#[test]
fn lua_keymap_operator_pending_o_mode_fires() {
    let mut e = engine_with_plugin(
        "hello world\n",
        "keymap_o_mode",
        r#"
        vimcode.keymap.set("o", "Q", function()
            vimcode.message("o-mode Q fired")
        end)
        "#,
    );
    press(&mut e, 'd'); // enter operator-pending (delete)
    press(&mut e, 'Q'); // Lua o-mode map, not a built-in motion
    assert_eq!(
        e.message, "o-mode Q fired",
        "an 'o' mode map must be consulted while an operator is pending"
    );
}

#[test]
fn lua_keymap_visual_x_mode_fires() {
    let mut e = engine_with_plugin(
        "hello world\n",
        "keymap_x_mode",
        r#"
        vimcode.keymap.set("x", "Q", function()
            vimcode.message("x-mode Q fired")
        end)
        "#,
    );
    press(&mut e, 'v'); // enter Visual mode
    press(&mut e, 'Q');
    assert_eq!(
        e.message, "x-mode Q fired",
        "an 'x' mode map must fire in Visual mode"
    );
}

#[test]
fn lua_keymap_list_enumerates_mode_lhs_buffer_and_desc() {
    let mut e = engine_with_plugin(
        "hello\n",
        "keymap_list",
        r#"
        vimcode.keymap.set("n", "gy", function() end, {desc = "yank stuff"})
        vimcode.command("ListIt", function(_)
            for _, m in ipairs(vimcode.keymap.list()) do
                if m.lhs == "gy" then
                    vimcode.message(
                        "mode=" .. m.mode
                        .. " desc=" .. (m.desc or "nil")
                        .. " buffer=" .. tostring(m.buffer)
                    )
                end
            end
        end)
        "#,
    );
    exec(&mut e, "ListIt");
    assert_eq!(
        e.message, "mode=n desc=yank stuff buffer=nil",
        "vimcode.keymap.list() must surface mode, lhs, desc and buffer-locality"
    );
}

// ── g@ operator marks (#1623) ───────────────────────────────────────────────

#[test]
fn g_at_operator_sets_open_and_close_marks_charwise() {
    let mut e = engine_with_plugin(
        "hello world\n",
        "g_at_charwise",
        r#"
        vimcode.set_operatorfunc(function(_)
            local a = vimcode.state.mark("[")
            local b = vimcode.state.mark("]")
            vimcode.message(
                "open=" .. a.line .. "," .. a.col
                .. " close=" .. b.line .. "," .. b.col
            )
        end)
        "#,
    );
    // g@w: operate on a charwise "word" motion from the start of the buffer.
    e.feed_keys("g@w");
    assert_eq!(
        e.message, "open=1,1 close=1,6",
        "g@ must set '[ / '] to the motion's charwise span before calling \
         the operatorfunc"
    );
}

#[test]
fn g_at_operator_sets_open_and_close_marks_linewise() {
    let mut e = engine_with_plugin(
        "line one\nline two\nline three\n",
        "g_at_linewise",
        r#"
        vimcode.set_operatorfunc(function(_)
            local a = vimcode.state.mark("[")
            local b = vimcode.state.mark("]")
            vimcode.message(
                "open=" .. a.line .. "," .. a.col
                .. " close=" .. b.line .. "," .. b.col
            )
        end)
        "#,
    );
    // g@j: linewise motion spanning the first two lines.
    e.feed_keys("g@j");
    assert_eq!(
        e.message, "open=1,1 close=2,9",
        "g@ must set '[ / '] to the motion's linewise span (first line's \
         start, last line's last column) before calling the operatorfunc"
    );
}

// ── Buffer write hook (#1623, oil.nvim shape) ───────────────────────────────

#[test]
fn buffer_write_handler_takes_over_save_and_clears_dirty() {
    let mut e = engine_with_plugin(
        "hello\n",
        "write_handler",
        r#"
        vimcode.command("SetupOil", function(_)
            local b = vimcode.buffer.create({scratch = true, name = "oil"})
            vimcode.buffer.set_lines(b, 0, -1, {"line-from-plugin"})
            vimcode.buffer.set_write_handler(b, function(buf)
                vimcode.message("wrote:" .. buf)
            end)
            vimcode.window.set_buf(0, b)
        end)
        "#,
    );
    exec(&mut e, "SetupOil");
    let buf_id = e.active_buffer_id();
    assert!(
        e.buffer_manager.get(buf_id).unwrap().dirty,
        "set_lines must have left the scratch buffer dirty"
    );
    assert!(
        e.save().is_ok(),
        "save() must succeed via the write handler"
    );
    assert_eq!(
        e.message,
        format!("wrote:{}", buf_id.0),
        "the write handler must run with the buffer's own handle, instead of \
         vimcode writing to disk"
    );
    assert!(
        !e.buffer_manager.get(buf_id).unwrap().dirty,
        "a successful plugin write must clear the dirty flag"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Plugin API — Phase 3 (#1624): loop timers, schedule/defer, callback
// registry, spawn with streamed output + exit status + kill.
// ═══════════════════════════════════════════════════════════════════════════
//
// Timers/schedule/defer are driven purely by `Engine::poll_idle()` — there is
// no background thread involved, so polling in a tight loop with a small
// bounded sleep (never a fixed-duration sleep-then-assert-once) is both
// deterministic and fast. `loop.spawn` genuinely does run a real child
// process on background threads, so those tests use the same poll-until-
// condition-or-timeout shape with a generous (but bounded) deadline rather
// than depending on wall-clock timing to land a specific chunk count.

/// Poll `e.poll_idle()` (and let `cond` run its own `:Command` reads against
/// the same `&mut Engine`) until `cond(e)` is true or `timeout` elapses.
/// Returns whether the condition was met — callers still assert on the
/// *result*, per this repo's "assert on rendered/observed output" rule; this
/// is purely a scheduling helper, never itself the assertion.
fn poll_until(
    e: &mut vimcode_core::Engine,
    timeout: std::time::Duration,
    mut cond: impl FnMut(&mut vimcode_core::Engine) -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        e.poll_idle();
        if cond(e) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return cond(e);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Absolute path to a standard POSIX utility, found by scanning the
/// conventional system `bin` directories instead of consulting `PATH`.
///
/// **A spawn test in this file must never let a shell resolve a utility via
/// `PATH`.** `cargo test` runs one test binary's tests as parallel threads of
/// a *single* process, and several tests in this very file deliberately
/// mutate the process-global `PATH` while they run: the `#1345`
/// managed-tool-resolver group sets it to `""` (see
/// `resolve_command_finds_managed_tool_binary_with_empty_path` and friends)
/// and the `#917` Homebrew group sets it to `/usr/bin:/bin`. Their
/// `EnvVarGuard`s restore it afterwards and their own mutex serializes them
/// against each other, but neither can stop an *unrelated* concurrent test
/// from spawning a child inside that window — and the child inherits the
/// emptied `PATH`.
///
/// That is exactly how
/// `spawn_flood_defers_overflow_to_next_tick_without_dropping_chunks` failed
/// in a full-suite run while passing in isolation (#1627 test stage): with
/// `PATH=""` its `/bin/sh -c "head -c … | tr …"` child died instantly with
/// `head: not found`, exit 127, zero bytes of stdout — so the test then
/// polled fruitlessly for its whole 20s deadline. Baking absolute paths into
/// the shell snippet removes the dependency, and with it the flake.
#[cfg(unix)]
fn posix_tool(name: &str) -> String {
    for dir in ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
        let candidate = std::path::Path::new(dir).join(name);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    panic!(
        "this test needs the POSIX utility `{name}`, which was not found in \
         any standard system bin directory"
    );
}

#[test]
fn loop_timer_fires_repeatedly_and_stop_prevents_further_calls() {
    let mut e = engine_with_plugin(
        "",
        "timer_repeat_1624",
        r#"
        _G.count = 0
        vimcode.command("StartTimer", function(_)
            _G.handle = vimcode.loop.timer(5, function()
                _G.count = _G.count + 1
                vimcode.message("ticks=" .. _G.count)
            end, { ["repeat"] = true })
        end)
        vimcode.command("StopTimer", function(_)
            _G.handle.stop()
        end)
        "#,
    );
    exec(&mut e, "StartTimer");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(2), |e| {
        e.message == "ticks=3"
    });
    assert!(
        reached,
        "a repeating 5ms timer must have fired at least 3 times within 2s; \
         last message: {:?}",
        e.message
    );

    exec(&mut e, "StopTimer");
    let msg_at_stop = e.message.clone();
    // No further ticks should land, however long we keep polling — this is
    // the RED-able half: without `stop()` actually removing the entry, this
    // loop would see `ticks=4`, `ticks=5`, ... within the same window.
    for _ in 0..30 {
        e.poll_idle();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        e.message, msg_at_stop,
        "stop() must prevent any further ticks from landing"
    );
}

#[test]
fn schedule_and_defer_run_in_registration_and_delay_order() {
    let mut e = engine_with_plugin(
        "",
        "schedule_order_1624",
        r#"
        _G.order = {}
        vimcode.command("RunSchedule", function(_)
            vimcode.schedule(function() table.insert(_G.order, "a") end)
            vimcode.schedule(function() table.insert(_G.order, "b") end)
            vimcode.defer(30, function() table.insert(_G.order, "d") end)
            vimcode.schedule(function() table.insert(_G.order, "c") end)
        end)
        vimcode.command("ReadOrder", function(_)
            vimcode.message(table.concat(_G.order, ","))
        end)
        "#,
    );
    exec(&mut e, "RunSchedule");

    // All three `schedule()`s are due "now"; the very next idle tick must
    // run them in the order they were registered, before the later-due
    // `defer()`. RED-able: a `HashMap`-iteration-order implementation (no
    // explicit tie-break) would flake between "a,b,c" and any permutation.
    e.poll_idle();
    exec(&mut e, "ReadOrder");
    assert_eq!(
        e.message, "a,b,c",
        "schedule() callbacks must fire in registration order on the next \
         idle tick, before defer()'s later-due callback"
    );

    let reached = poll_until(&mut e, std::time::Duration::from_secs(2), |e| {
        exec(e, "ReadOrder");
        e.message == "a,b,c,d"
    });
    assert_eq!(
        e.message, "a,b,c,d",
        "defer() must run after its own delay, after every schedule()"
    );
    assert!(reached);
}

#[test]
fn timer_callback_edits_buffer_via_immediate_api() {
    let mut e = engine_with_plugin(
        "original\n",
        "timer_edit_buffer_1624",
        r#"
        vimcode.command("ScheduleEdit", function(_)
            vimcode.defer(5, function()
                local b = vimcode.buffer.create({ scratch = true, name = "ZQ1624TIMER" })
                vimcode.buffer.set_lines(b, 0, -1, { "EDITED_BY_TIMER_1624" })
                vimcode.window.set_buf(0, b)
            end)
        end)
        "#,
    );
    exec(&mut e, "ScheduleEdit");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(2), |e| {
        e.buffer().to_string().contains("EDITED_BY_TIMER_1624")
    });
    assert!(
        reached,
        "a deferred callback must be able to use the immediate \
         vimcode.buffer API (create/set_lines/window.set_buf); buffer: {:?}",
        e.buffer().to_string()
    );
}

#[test]
#[cfg(unix)]
fn concurrent_spawns_stream_independent_output_and_exit_codes() {
    let mut e = engine_with_plugin(
        "",
        "spawn_concurrent_1624",
        r#"
        _G.out_a = ""
        _G.out_b = ""
        _G.exit_a = nil
        _G.exit_b = nil
        vimcode.command("RunSpawns", function(_)
            vimcode.loop.spawn("/bin/sh", { "-c", "printf AAA" }, {
                on_stdout = function(chunk) _G.out_a = _G.out_a .. chunk end,
                on_exit = function(code, _signal) _G.exit_a = code end,
            })
            vimcode.loop.spawn("/bin/sh", { "-c", "printf BBB" }, {
                on_stdout = function(chunk) _G.out_b = _G.out_b .. chunk end,
                on_exit = function(code, _signal) _G.exit_b = code end,
            })
        end)
        vimcode.command("ReadSpawns", function(_)
            vimcode.message(_G.out_a .. "|" .. _G.out_b .. "|"
                .. tostring(_G.exit_a) .. "|" .. tostring(_G.exit_b))
        end)
        "#,
    );
    exec(&mut e, "RunSpawns");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(15), |e| {
        exec(e, "ReadSpawns");
        e.message == "AAA|BBB|0|0"
    });
    assert!(
        reached,
        "each spawn must receive its own streamed stdout and exit code, \
         unclobbered by the other; last message: {:?}",
        e.message
    );
}

#[test]
#[cfg(unix)]
fn spawn_kill_delivers_nonzero_or_signalled_exit() {
    // Absolute `sleep`, not a `PATH` lookup: with a concurrent test's
    // `PATH=""` in effect the child would exit 127 immediately instead of
    // living long enough to be killed, and this would pass for the wrong
    // reason (127 is "nonzero") — see `posix_tool`.
    let sleep = posix_tool("sleep");
    // `exec` matters, and is the second half of this test's flake fix
    // (#1627 test stage): `/bin/sh -c "sleep 30"` *forks* a grandchild for
    // the `sleep` on the shells this runs on, and `h.kill()` signals only
    // the direct child. If the kill lands after that fork — which is
    // exactly what a loaded machine makes likely — the orphaned `sleep`
    // keeps the child's stdout pipe write end open, so the reader threads
    // never see EOF, `execute::finish_and_send_exit` is never reached and
    // *no* `Exit` event is ever sent: this test then burned its full 15s
    // deadline and failed ("kill() must eventually deliver an exit"), which
    // reproduced in ~half of 15 loaded runs of this binary. `exec` makes
    // the shell *become* `sleep`, so there is exactly one process to kill.
    //
    // NOTE: that a grandchild survives `handle.kill()` (and with it
    // suppresses `on_exit` for good) is a real limitation of
    // `vimcode.loop.spawn` — it kills a pid, not a process group. Worth a
    // follow-up issue; deliberately *not* papered over here beyond keeping
    // this test about the single-child case it was written for.
    let mut e = engine_with_plugin(
        "",
        "spawn_kill_1624",
        &format!(
            r#"
        _G.exit_code = nil
        _G.exit_signal = nil
        vimcode.command("RunAndKill", function(_)
            local h = vimcode.loop.spawn("/bin/sh", {{ "-c", "exec {sleep} 30" }}, {{
                on_exit = function(code, signal)
                    _G.exit_code = code
                    _G.exit_signal = signal
                end,
            }})
            h.kill()
        end)
        vimcode.command("ReadExit", function(_)
            vimcode.message(tostring(_G.exit_code) .. "/" .. tostring(_G.exit_signal))
        end)
        "#
        ),
    );
    exec(&mut e, "RunAndKill");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(15), |e| {
        exec(e, "ReadExit");
        e.message != "nil/nil"
    });
    assert!(reached, "kill() must eventually deliver an exit to on_exit");
    assert_ne!(
        e.message, "0/nil",
        "a killed process must not report a clean, unsignalled exit; got {:?}",
        e.message
    );
}

#[test]
#[cfg(unix)]
fn async_shell_reimplemented_on_spawn_keeps_legacy_behaviour_and_gains_exit_status() {
    let mut e = engine_with_plugin(
        "",
        "async_shell_exit_1624",
        r#"
        vimcode.command("RunShell", function(_)
            vimcode.async_shell("printf hello", "shell_done_1624")
        end)
        vimcode.on("shell_done_1624", function(output)
            local code = vimcode.async_shell_exit_code("shell_done_1624")
            vimcode.message(output .. "/" .. tostring(code))
        end)
        "#,
    );
    exec(&mut e, "RunShell");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(15), |e| {
        e.message == "hello/0"
    });
    assert!(
        reached,
        "async_shell must still deliver its output via plugin_event (the \
         frozen legacy contract), and vimcode.async_shell_exit_code must see \
         the just-landed exit code from inside the very callback the event \
         fires; last message: {:?}",
        e.message
    );
}

/// #1624 review (blocking): `vimcode.async_shell`'s optional `stdin` write
/// must not block the calling thread — the engine/main thread when this
/// runs for real, and this test's own thread here, since `exec()` dispatches
/// the command (and, inline within the same call, `Engine::apply_plugin_ctx`
/// -> `execute::spawn_piped`) synchronously.
///
/// The child (`sleep 0.5 && cat`) deliberately does not read a single byte
/// of stdin for 500ms, and the written payload (256KiB) is comfortably
/// larger than the ~64KiB default Linux pipe buffer — so a `write_all` that
/// ran inline on the caller's thread would block for a large fraction of
/// that 500ms waiting for the child to start draining. `exec()` returning
/// in well under that (a generous 200ms budget) is only possible if the
/// write happens on its own background thread instead.
///
/// RED-verified against `execute::spawn_piped` reverted to write inline
/// (`pipe.write_all` called directly on `stdin.as_mut()` before returning,
/// instead of moving the pipe into a spawned thread): this does not merely
/// run slower, it **deadlocks outright** and had to be killed after
/// exceeding a 120s bound — the inline write blocks this thread before the
/// stdout/stderr reader threads further down `spawn_piped` ever get spawned,
/// so `cat`'s own stdout pipe fills up with nowhere to drain to, and `cat`
/// blocks writing it while we're blocked writing its stdin. Exactly the
/// classic pipe deadlock the review named, reproduced for real.
#[test]
#[cfg(unix)]
fn async_shell_large_stdin_write_does_not_block_the_calling_thread() {
    let big_stdin = "A".repeat(256 * 1024);
    // Absolute `sleep`/`cat` — under a concurrent test's `PATH=""` the shell
    // would fail to find either, exit 127 and echo nothing back, failing the
    // `len=` assertion below for a reason that has nothing to do with where
    // the stdin write runs. See `posix_tool`.
    let sleep = posix_tool("sleep");
    let cat = posix_tool("cat");
    let mut e = engine_with_plugin(
        "",
        "async_shell_big_stdin_1624",
        &format!(
            r#"
        _G.big_stdin = string.rep("A", 256 * 1024)
        vimcode.command("RunBigStdin", function(_)
            vimcode.async_shell("{sleep} 0.5 && {cat}", "big_stdin_done_1624", {{ stdin = _G.big_stdin }})
        end)
        vimcode.on("big_stdin_done_1624", function(output)
            vimcode.message("len=" .. #output)
        end)
        "#
        ),
    );

    let before = std::time::Instant::now();
    exec(&mut e, "RunBigStdin");
    let dispatch_elapsed = before.elapsed();
    assert!(
        dispatch_elapsed < std::time::Duration::from_millis(200),
        "dispatching :RunBigStdin must return promptly — the {}-byte stdin \
         write must happen off the calling thread, not block it while the \
         child (which sleeps 500ms before reading anything) drains it; \
         actual elapsed: {dispatch_elapsed:?}",
        big_stdin.len()
    );

    let reached = poll_until(&mut e, std::time::Duration::from_secs(15), |e| {
        e.message == format!("len={}", big_stdin.len())
    });
    assert!(
        reached,
        "the large stdin payload must still fully reach the child and be \
         echoed back once it starts draining, even though the write ran \
         off-thread; last message: {:?}",
        e.message
    );
}

#[test]
#[cfg(unix)]
fn unloading_plugin_stops_its_timers_and_spawns_from_calling_back() {
    // Absolute `sleep` — a `PATH=""` window from a concurrent test would
    // make this child die with `sleep: not found` before the unload, so the
    // "no callbacks after unload" assertion would no longer be testing the
    // liveness check it exists for. See `posix_tool`.
    let sleep = posix_tool("sleep");
    let mut e = engine_with_plugin(
        "",
        "unload_cancels_1624",
        &format!(
            r#"
        vimcode.command("StartAll", function(_)
            vimcode.loop.timer(5, function()
                vimcode.message("tick")
            end, {{ ["repeat"] = true }})
            vimcode.loop.spawn("/bin/sh", {{ "-c", "exec {sleep} 30" }}, {{
                on_stdout = function(chunk) vimcode.message("spawn:" .. chunk) end,
            }})
        end)
        "#
        ),
    );
    exec(&mut e, "StartAll");

    // Confirm the timer is actually live before unloading (otherwise "no
    // more ticks after unload" would trivially pass for the wrong reason).
    let ticking = poll_until(&mut e, std::time::Duration::from_secs(2), |e| {
        e.message == "tick"
    });
    assert!(ticking, "setup: the timer must be running before unload");

    // "Unload" = install a fresh `PluginManager` (`Engine::
    // set_plugin_manager`'s doc: this is the mechanism, an `Rc` swap that
    // drops the old plugin's Lua state and every registry key it owned).
    let fresh = vimcode_core::core::plugin::PluginManager::new().unwrap();
    e.set_plugin_manager(fresh);

    // `message` is engine-level (Rust) state, untouched by the manager swap
    // itself, so any further change to it can only come from a callback
    // that actually ran — which must not happen once its owning manager is
    // gone. RED-able: without the `Weak`/liveness check in
    // `poll_plugin_timers`/`poll_plugin_spawns`, this would panic (calling
    // into a `LuaRegistryKey` that belongs to a dropped `Lua` state) or, if
    // it merely leaked, would still print "tick"/"spawn:..." here.
    e.message = "SENTINEL_1624".to_string();
    for _ in 0..40 {
        e.poll_idle();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        e.message, "SENTINEL_1624",
        "no timer/spawn callback belonging to the unloaded plugin may fire \
         after it is replaced"
    );
}

/// Whether the process named `pid` is still alive, probed with `kill -0`
/// through `/bin/sh` (same reasoning as this file's other spawn tests
/// already depending on `/bin/sh` being present) rather than pulling in a
/// signals crate just for this one check.
#[cfg(unix)]
fn pid_is_alive(pid: i32) -> bool {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("kill -0 {pid} 2>/dev/null"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Build a bare [`vimcode_core::core::plugin::PluginManager`] (not yet
/// installed on any `Engine`) with `lua_code` loaded from a temp dir named
/// `plugin_name`. Companion to [`engine_with_plugin`] for tests that need to
/// control exactly when the manager is installed (here: to land it and its
/// very first `vimcode.loop.spawn` registration with nothing — not even one
/// `poll_idle` tick — in between).
#[cfg(unix)]
fn plugin_manager_with(
    plugin_name: &str,
    lua_code: &str,
) -> vimcode_core::core::plugin::PluginManager {
    let dir = plugin_fixture_dir(plugin_name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{plugin_name}.lua")), lua_code).unwrap();
    let mut mgr = vimcode_core::core::plugin::PluginManager::new().expect("PluginManager::new");
    mgr.load_plugins_dir(&dir, &[]);
    mgr
}

/// #1624 review (blocking): a manager swap must not let the id-0 collision
/// between the outgoing and incoming `PluginManager` generations
/// (`PluginManager::next_handle_id` restarts at 0 for every fresh manager,
/// while `Engine::plugin_spawns` is one `HashMap` that outlives any single
/// generation) silently drop a still-live `vimcode.loop.spawn` handle
/// without ever calling `child.kill()` on it.
///
/// Reproduces the exact sequence the review describes: manager A registers
/// a spawn (getting id 0), then — with **no intervening `poll_idle` tick**,
/// so the old purely-lazy `Weak`-upgrade cleanup in `poll_plugin_timers`/
/// `poll_plugin_spawns` never gets a chance to run first — manager B is
/// installed and immediately registers its own first spawn, which also
/// gets id 0 and lands in the very same `HashMap` slot.
///
/// Manager A's child is a `while :; do :; done` busy-loop built entirely of
/// shell builtins: it never forks a separate grandchild (so `kill()`ing the
/// direct child genuinely stops everything — a plain PID-liveness probe is
/// the correct black-box signal here) and, unlike a child blocked reading
/// its own stdin, it does not exit merely because the engine-held
/// `ChildStdin` end happens to get dropped when its `PluginSpawnHandle` is
/// overwritten — it only stops when actually killed, which is exactly the
/// distinction this test needs to catch a silent map-slot overwrite. If the
/// id-0 slot is silently overwritten instead of reaped first, that child is
/// never killed and keeps running for as long as this whole test process
/// does.
///
/// RED-verified: with `Engine::set_plugin_manager`'s synchronous
/// `reap_stale_plugin_timers_and_spawns` call removed (restoring the
/// purely-lazy, poll-tick-only cleanup this issue's review flagged), this
/// fails — manager A's child is still alive at the end of the bounded wait,
/// because manager B's spawn insert silently replaced the map slot before
/// anything ever reaped A's entry.
#[test]
#[cfg(unix)]
fn manager_swap_id_collision_does_not_leak_orphaned_spawn_process() {
    let pidfile =
        std::env::temp_dir().join(format!("vc_1624_collision_pid_{}", std::process::id()));
    let _ = std::fs::remove_file(&pidfile);

    let mut e = engine_with_plugin(
        "",
        "spawn_collision_a_1624",
        &format!(
            r#"
            vimcode.command("StartOrphan", function(_)
                vimcode.loop.spawn("/bin/sh", {{ "-c", "echo $$ > {path}; while :; do :; done" }})
            end)
            "#,
            path = pidfile.display()
        ),
    );
    exec(&mut e, "StartOrphan");

    let wrote = poll_until(&mut e, std::time::Duration::from_secs(5), |_| {
        pidfile.exists()
    });
    assert!(
        wrote,
        "setup: manager A's spawned child must record its own pid before \
         the manager swap"
    );
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .expect("child must have written a plain integer pid");
    assert!(
        pid_is_alive(pid),
        "setup: manager A's spawned child must actually be running before \
         the swap"
    );

    // Install manager B and have it register its own first spawn (id 0,
    // colliding with A's still-live id-0 entry) with *no* `e.poll_idle()`
    // call anywhere in between — the exact window the review's finding
    // describes.
    let mgr_b = plugin_manager_with(
        "spawn_collision_b_1624",
        r#"
        vimcode.command("StartNew", function(_)
            vimcode.loop.spawn("/bin/sh", { "-c", "true" })
        end)
        "#,
    );
    e.set_plugin_manager(mgr_b);
    exec(&mut e, "StartNew");

    // `kill()` itself is synchronous, but the OS reaping a signalled
    // process is not instantaneous — bounded poll, not a fixed sleep.
    let dead = poll_until(&mut e, std::time::Duration::from_secs(2), |_| {
        !pid_is_alive(pid)
    });
    assert!(
        dead,
        "manager A's spawned child (pid {pid}) must be killed when its \
         owning plugin manager is replaced, even though manager B's own \
         first spawn reused the exact same handle id"
    );

    let _ = std::fs::remove_file(&pidfile);
}

/// A child that floods stdout produces far more `PluginSpawnEvent`s than the
/// `MAX_SPAWN_EVENTS_PER_TICK` budget one `poll_idle` tick will process. The
/// budget must *defer* the overflow to the next tick, never drop it: the
/// plugin has to observe every single byte the child wrote, plus the exit.
///
/// RED against the first version of the per-tick cap (which called
/// `try_recv()` in the `while let` condition and only then checked the
/// budget, so the event already popped off the channel was discarded by the
/// `break`): that reported 1995904/0 instead of 2000000/0 — one lost 4KiB
/// chunk per capped tick.
#[test]
#[cfg(unix)]
fn spawn_flood_defers_overflow_to_next_tick_without_dropping_chunks() {
    const EXPECTED_BYTES: usize = 2_000_000;

    // Absolute paths, never `PATH` lookups — see `posix_tool`'s doc comment
    // for the concurrent-`PATH=""` flake this avoids (#1627 test stage).
    let head = posix_tool("head");
    let tr = posix_tool("tr");

    let mut e = engine_with_plugin(
        "",
        "spawn_flood_1624",
        &format!(
            r#"
            _G.total = 0
            _G.exit_code = nil
            vimcode.command("RunFlood", function(_)
                vimcode.loop.spawn("/bin/sh",
                    {{ "-c", "{head} -c {bytes} /dev/zero | {tr} '\\0' x" }}, {{
                    on_stdout = function(chunk) _G.total = _G.total + #chunk end,
                    on_exit = function(code, _signal) _G.exit_code = code end,
                }})
            end)
            vimcode.command("ReadFlood", function(_)
                vimcode.message(string.format("%d/%s", _G.total,
                    tostring(_G.exit_code)))
            end)
            "#,
            bytes = EXPECTED_BYTES
        ),
    );
    exec(&mut e, "RunFlood");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(20), |e| {
        exec(e, "ReadFlood");
        e.message == format!("{EXPECTED_BYTES}/0")
    });
    assert!(
        reached,
        "a spawn whose output exceeds one tick's event budget must have every \
         chunk delivered on later ticks (and its exit delivered at all); \
         expected {EXPECTED_BYTES}/0, last message: {:?}",
        e.message
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Plugin API — Phase 4 (#1630): `vimcode.picker.open` — open the built-in
// picker from Lua with plugin items, stored-callback actions (`on_select`/
// `on_cancel`), live/async item updates (`:append`), and file/buffer
// preview. Maps to `PickerSource::Custom("plugin:<id>")` /
// `PickerAction::Custom("plugin_item:<id>:<item_id>")` under the hood — see
// `src/core/engine/plugins.rs`'s `plugin_api_picker_*` methods.
// ═══════════════════════════════════════════════════════════════════════════

/// #1630 acceptance: opening with a static item list must populate the
/// picker's title and items immediately — no callback round-trip needed for
/// the simplest case.
///
/// RED-verified against unfixed `develop`: there is no `vimcode.picker`
/// table there, so the `Open` command errors at its first line and
/// `e.picker_open` stays `false`.
#[test]
fn picker_open_with_static_items_shows_them() {
    let mut e = engine_with_plugin(
        "",
        "picker_open_1630",
        r#"
        vimcode.command("Open", function(_)
            vimcode.picker.open({
                title = "My Picker",
                items = {
                    { display = "alpha", data = "A" },
                    { display = "beta", data = "B" },
                },
            })
        end)
        "#,
    );
    exec(&mut e, "Open");
    assert!(
        e.picker_open,
        "vimcode.picker.open must open the unified picker"
    );
    assert_eq!(e.picker_title, "My Picker");
    let displays: Vec<&str> = e.picker_items.iter().map(|i| i.display.as_str()).collect();
    assert_eq!(displays, vec!["alpha", "beta"]);
}

/// #1630 acceptance: confirming an item fires `on_select` with that item's
/// `data`, unmodified, and closes the picker.
///
/// RED-verified against unfixed `develop`: same as above — `on_select`
/// never registers, so `_G.selected` stays `nil` and `Read` reports `nil`.
#[test]
fn picker_select_invokes_on_select_with_item_data() {
    let mut e = engine_with_plugin(
        "",
        "picker_select_1630",
        r#"
        _G.selected = nil
        vimcode.command("Open", function(_)
            vimcode.picker.open({
                items = { { display = "alpha", data = "payload-A" } },
                on_select = function(data) _G.selected = data end,
            })
        end)
        vimcode.command("Read", function(_)
            vimcode.message(tostring(_G.selected))
        end)
        "#,
    );
    exec(&mut e, "Open");
    assert!(e.picker_open, "precondition: the picker is open");

    press_key(&mut e, "Return");

    assert!(!e.picker_open, "confirming an item must close the picker");
    exec(&mut e, "Read");
    assert_eq!(
        e.message, "payload-A",
        "on_select must receive the confirmed item's `data` verbatim"
    );
}

/// #1630 acceptance: pressing Escape on a plugin-owned picker fires
/// `on_cancel` (and only `on_cancel` — `on_select` must not also fire).
///
/// RED-verified against unfixed `develop`: `on_cancel` never registers, so
/// `_G.cancelled` stays `false` and `Read` reports `"false"`.
#[test]
fn picker_cancel_invokes_on_cancel() {
    let mut e = engine_with_plugin(
        "",
        "picker_cancel_1630",
        r#"
        _G.cancelled = false
        _G.selected = false
        vimcode.command("Open", function(_)
            vimcode.picker.open({
                items = { { display = "alpha" } },
                on_select = function(_data) _G.selected = true end,
                on_cancel = function() _G.cancelled = true end,
            })
        end)
        vimcode.command("Read", function(_)
            vimcode.message(tostring(_G.cancelled) .. "/" .. tostring(_G.selected))
        end)
        "#,
    );
    exec(&mut e, "Open");

    press_key(&mut e, "Escape");

    assert!(!e.picker_open, "Escape must close the picker");
    exec(&mut e, "Read");
    assert_eq!(
        e.message, "true/false",
        "Escape must fire on_cancel, never on_select"
    );
}

/// #1630 acceptance: `:append(items)` on a still-open picker extends the
/// item list and re-filters against whatever query the user has already
/// typed — a streamed source (e.g. one fed by a `vimcode.loop.spawn`
/// `on_stdout`) can grow the list live without the user's typing being
/// interrupted or the match set going stale.
///
/// RED-verified against unfixed `develop`: `vimcode.picker` doesn't exist,
/// so `Open` errors immediately and `e.picker_items` stays empty.
#[test]
fn picker_append_while_open_extends_the_filtered_list() {
    let mut e = engine_with_plugin(
        "",
        "picker_append_1630",
        r#"
        _G.handle = nil
        vimcode.command("Open", function(_)
            _G.handle = vimcode.picker.open({
                items = {
                    { display = "apple" },
                    { display = "apricot" },
                    { display = "banana" },
                },
            })
        end)
        vimcode.command("Append", function(_)
            _G.handle:append({ { display = "grape" } })
        end)
        "#,
    );
    exec(&mut e, "Open");

    // Filter to "ap" — matches "apple"/"apricot" (subsequence a-then-p);
    // "banana" has no 'p' at all and must stay filtered out throughout.
    type_chars(&mut e, "ap");
    let before: Vec<&str> = e.picker_items.iter().map(|i| i.display.as_str()).collect();
    assert_eq!(
        before,
        vec!["apple", "apricot"],
        "precondition: typing must fuzzy-filter the static items first"
    );

    exec(&mut e, "Append");

    let after: Vec<&str> = e.picker_items.iter().map(|i| i.display.as_str()).collect();
    assert!(
        after.contains(&"grape"),
        "append must extend the list and re-filter the new item against the \
         current query (\"grape\" is a-then-p subsequence match for \"ap\"): {after:?}"
    );
    assert!(
        !after.contains(&"banana"),
        "an item that doesn't match the current query must stay filtered \
         out after append: {after:?}"
    );
}

/// #1630 acceptance: an item whose `preview` names a buffer handle (+ line)
/// populates the picker's preview pane from that buffer's *live* content —
/// not disk — windowed around the given line the same way a file-based
/// preview centers on a match.
///
/// RED-verified against unfixed `develop`: `vimcode.picker` doesn't exist,
/// so `e.picker_preview` stays `None`.
#[test]
fn picker_preview_shows_buffer_handle_content_at_the_given_line() {
    let mut e = engine_with_plugin(
        "",
        "picker_preview_buf_1630",
        r#"
        vimcode.command("Open", function(_)
            local b = vimcode.buffer.create({ scratch = true, name = "PREVBUF" })
            vimcode.buffer.set_lines(b, 0, -1, { "first", "second", "third" })
            vimcode.picker.open({
                items = {
                    { display = "item one", preview = { buffer = b, line = 2 } },
                },
            })
        end)
        "#,
    );
    exec(&mut e, "Open");
    assert!(e.picker_open, "precondition: the picker is open");

    let preview = e
        .picker_preview
        .as_ref()
        .expect("a buffer-preview item must populate the preview pane");
    let texts: Vec<&str> = preview.lines.iter().map(|(_, t, _)| t.as_str()).collect();
    assert!(
        texts.contains(&"second"),
        "the previewed buffer's live content must appear in the preview \
         pane: {texts:?}"
    );
    assert!(
        preview
            .lines
            .iter()
            .any(|(lineno, _, is_match)| *lineno == 2 && *is_match),
        "line 2 (1-indexed, as declared) must be marked as the match line: \
         {:?}",
        preview.lines
    );
}

/// #1630 acceptance: unloading the plugin that owns an open picker (a fresh
/// `Engine::set_plugin_manager`, the same mechanism `:PluginDisable`/
/// extension uninstall use) closes it — the same "no calls into an
/// unloaded plugin, and no stray UI left behind" contract #1624 already
/// gives timers and spawns.
///
/// RED-verified against unfixed `develop`: `vimcode.picker` doesn't exist
/// (so this would panic on `Open` before ever reaching the unload half);
/// on a `develop` with just `PickerSource::Custom` still unused, nothing
/// would ever close a stale picker on unload since there is no reap logic
/// wired to it at all.
#[test]
fn picker_unload_closes_open_picker() {
    let mut e = engine_with_plugin(
        "",
        "picker_unload_1630",
        r#"
        vimcode.command("Open", function(_)
            vimcode.picker.open({ items = { { display = "alpha" } } })
        end)
        "#,
    );
    exec(&mut e, "Open");
    assert!(e.picker_open, "precondition: the picker is open");

    let empty =
        vimcode_core::core::plugin::PluginManager::new().expect("PluginManager::new must succeed");
    e.set_plugin_manager(empty);

    assert!(
        !e.picker_open,
        "unloading the owning plugin must close its still-open picker"
    );
}

/// #1630 review (blocking): a plugin calling `vimcode.picker.open` again
/// while a previous handle from the same plugin is still open — the natural
/// telescope-style pattern (a command re-invoked on every keystroke) — must
/// not leave the old registration (`on_select`/`on_cancel`/`on_query` plus
/// every item's `data`) registered forever. `open_picker` blows away
/// `picker_all_items`/`picker_source` with no idea a plugin picker used to
/// own them, so `plugin_api_picker_open` must tear the old one down itself
/// before registering the new one.
///
/// RED-verified against this fix reverted (dropping the `plugin_picker_
/// teardown` call `plugin_api_picker_open` now makes for its own previous
/// registration): this fails with a registration count of `2` after the
/// second `Open`, proving the first handle's callbacks/item `data` are
/// still registered and permanently unreachable.
#[test]
fn picker_reopen_supersedes_without_leaking_old_registration() {
    let mut e = engine_with_plugin(
        "",
        "picker_reopen_1630",
        r#"
        vimcode.command("Open", function(_)
            vimcode.picker.open({
                items = { { display = "alpha" } },
                on_cancel = function() end,
            })
        end)
        "#,
    );
    exec(&mut e, "Open");
    assert_eq!(
        e.plugin_manager
            .as_ref()
            .unwrap()
            .picker_registration_count(),
        1,
        "precondition: the first Open registers exactly one picker"
    );

    exec(&mut e, "Open");

    assert!(e.picker_open, "the second Open must still open a picker");
    assert_eq!(
        e.plugin_manager
            .as_ref()
            .unwrap()
            .picker_registration_count(),
        1,
        "a fresh vimcode.picker.open call must tear down the previous \
         still-registered handle from the same plugin, not leak it \
         alongside the new one"
    );
}

/// #1630 review (blocking): a non-Escape path that dismisses a plugin-owned
/// picker (GTK's click-outside-to-dismiss, `render::PickerRoute::Dismiss` in
/// `src/app.rs`) must fire `on_cancel` and release the picker's registration
/// exactly like Escape does, instead of calling `close_picker()` directly
/// and leaking it forever. `Engine::close_picker_cancelling_plugin` is the
/// shared helper both Escape's `handle_picker_key` arm and that GTK path now
/// call through — exercised directly here since `tests/extensions.rs` has no
/// GTK click harness.
///
/// RED-verified against this fix reverted (restoring `close_picker_
/// cancelling_plugin` to a bare `self.close_picker()`): this fails on both
/// assertions — `on_cancel` never fires (`_G.cancelled` stays `false`) and
/// the registration count stays `1` instead of dropping to `0`.
#[test]
fn close_picker_cancelling_plugin_fires_on_cancel_and_releases_registration() {
    let mut e = engine_with_plugin(
        "",
        "picker_dismiss_1630",
        r#"
        _G.cancelled = false
        vimcode.command("Open", function(_)
            vimcode.picker.open({
                items = { { display = "alpha" } },
                on_cancel = function() _G.cancelled = true end,
            })
        end)
        vimcode.command("Read", function(_)
            vimcode.message(tostring(_G.cancelled))
        end)
        "#,
    );
    exec(&mut e, "Open");
    assert!(e.picker_open, "precondition: the picker is open");
    assert_eq!(
        e.plugin_manager
            .as_ref()
            .unwrap()
            .picker_registration_count(),
        1,
        "precondition: the picker is registered"
    );

    // Simulates the GTK click-outside-to-dismiss path (`render::
    // PickerRoute::Dismiss` in `src/app.rs`), which calls this exact
    // method instead of `close_picker()` directly.
    e.close_picker_cancelling_plugin();

    assert!(!e.picker_open, "dismissing must close the picker");
    exec(&mut e, "Read");
    assert_eq!(
        e.message, "true",
        "a non-Escape dismiss must fire on_cancel exactly like Escape does"
    );
    assert_eq!(
        e.plugin_manager
            .as_ref()
            .unwrap()
            .picker_registration_count(),
        0,
        "dismissing must release the picker's registration, not leak it"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Plugin API — Phase 2 (#1632): vimcode.http (async, curl-subprocess-backed,
// callback via poll_idle), vimcode.json, plugin-scoped storage.
// ═══════════════════════════════════════════════════════════════════════════

// ─── Local loopback HTTP fixture server ────────────────────────────────────
//
// `vimcode.http.request` genuinely shells out to `curl` on a background
// thread (see `execute::spawn_http_request`), so exercising it end-to-end
// needs a real socket on the other end — 127.0.0.1-only, an ephemeral port,
// nothing that reaches an external host. Same shape as `registry_liveness.rs`'s
// `spawn_head_fixture_server`, generalized to method/path/headers/body in and
// status/headers/body out so one server serves every scenario below.

use std::io::{Read as _, Write as _};

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Read one HTTP/1.1 request off `stream`: method, path, headers, and the
/// body (drained per `Content-Length`, defaulting to none).
fn read_http_fixture_request(
    stream: &mut std::net::TcpStream,
) -> Option<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 1_000_000 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Some((method, path, headers, body))
}

fn http_fixture_reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// Bind an ephemeral loopback port and serve up to `expected_requests`
/// connections, each answered by `handler(method, path, headers, body) ->
/// (status, headers, body)`. Returns the server's base URL immediately; the
/// server itself runs on a detached background thread bounded by a 30s
/// deadline (never joined — a test that cancels before the server responds
/// deliberately leaves it to finish out its own delay in the background,
/// same as `spawn_head_fixture_server`'s comment on why a wall-clock cap
/// matters more than a joinable handle here).
fn spawn_http_fixture_server(
    expected_requests: usize,
    handler: impl Fn(&str, &str, &[(String, String)], &[u8]) -> (u16, Vec<(String, String)>, Vec<u8>)
        + Send
        + 'static,
) -> String {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral loopback port");
    listener
        .set_nonblocking(true)
        .expect("set fixture listener nonblocking");
    let addr = listener.local_addr().expect("resolve bound local_addr");
    let base_url = format!("http://{addr}");
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut served = 0;
        while served < expected_requests && std::time::Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            let _ = stream.set_nonblocking(false);
            let Some((method, path, headers, body)) = read_http_fixture_request(&mut stream) else {
                continue;
            };
            let (status, resp_headers, resp_body) = handler(&method, &path, &headers, &body);
            let mut resp = format!(
                "HTTP/1.1 {status} {}\r\n",
                http_fixture_reason_phrase(status)
            );
            for (k, v) in &resp_headers {
                resp.push_str(&format!("{k}: {v}\r\n"));
            }
            resp.push_str(&format!("Content-Length: {}\r\n", resp_body.len()));
            resp.push_str("Connection: close\r\n\r\n");
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&resp_body);
            let _ = stream.flush();
            served += 1;
        }
    });
    base_url
}

// ─── `vimcode.http.request` ─────────────────────────────────────────────────

#[test]
fn http_request_get_returns_status_body_headers_and_elapsed_ms() {
    let base_url = spawn_http_fixture_server(1, |method, path, _headers, _body| {
        assert_eq!(method, "GET");
        assert_eq!(path, "/hello");
        (
            200,
            vec![("X-Foo".to_string(), "bar".to_string())],
            b"hello world".to_vec(),
        )
    });

    let mut e = engine_with_plugin(
        "",
        "http_get_1632",
        &format!(
            r#"
            _G.done = false
            vimcode.command("DoRequest", function(_)
                vimcode.http.request({{ method = "GET", url = "{base_url}/hello" }}, function(resp)
                    _G.status = resp.status
                    _G.body = resp.body
                    _G.foo = resp.headers["X-Foo"]
                    _G.elapsed_is_number = type(resp.elapsed_ms) == "number"
                    _G.has_error = resp.error ~= nil
                    _G.done = true
                end)
            end)
            vimcode.command("ReadResult", function(_)
                vimcode.message(table.concat({{
                    tostring(_G.done), tostring(_G.status), _G.body or "", _G.foo or "",
                    tostring(_G.elapsed_is_number), tostring(_G.has_error),
                }}, "|"))
            end)
            "#
        ),
    );
    exec(&mut e, "DoRequest");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(10), |e| {
        exec(e, "ReadResult");
        e.message.starts_with("true|")
    });
    assert!(
        reached,
        "the response callback must eventually fire; last message: {:?}",
        e.message
    );
    assert_eq!(
        e.message, "true|200|hello world|bar|true|false",
        "status/body/headers/elapsed_ms must all be delivered, and no error \
         field on a successful response"
    );
}

#[test]
fn http_request_post_sends_headers_and_body_the_server_receives() {
    let base_url = spawn_http_fixture_server(1, |method, path, headers, body| {
        assert_eq!(method, "POST");
        assert_eq!(path, "/echo");
        let got_header = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("x-plugin"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let resp = format!("{got_header}|{}", String::from_utf8_lossy(body));
        (200, vec![], resp.into_bytes())
    });

    let mut e = engine_with_plugin(
        "",
        "http_post_1632",
        &format!(
            r#"
            _G.done = false
            vimcode.command("DoRequest", function(_)
                vimcode.http.request({{
                    method = "POST",
                    url = "{base_url}/echo",
                    headers = {{ ["X-Plugin"] = "vc1632" }},
                    body = "payload-data",
                }}, function(resp)
                    _G.body = resp.body
                    _G.done = true
                end)
            end)
            vimcode.command("ReadResult", function(_)
                vimcode.message(tostring(_G.done) .. "|" .. (_G.body or ""))
            end)
            "#
        ),
    );
    exec(&mut e, "DoRequest");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(10), |e| {
        exec(e, "ReadResult");
        e.message.starts_with("true|")
    });
    assert!(
        reached,
        "response must arrive; last message: {:?}",
        e.message
    );
    assert_eq!(
        e.message, "true|vc1632|payload-data",
        "the server must receive exactly the header and body the plugin sent"
    );
}

/// #1632 review: a header value containing CR/LF must be rejected at the
/// `vimcode.http.request` boundary, never handed to `curl`'s `-H "{k}: {v}"`
/// argument — otherwise it smuggles an extra header line (or splits the
/// request) onto the wire. This drives it through the real Lua entry point
/// (not an internal helper) and confirms the server the request would have
/// hit never even sees a connection, because the request must fail before
/// `curl` is ever spawned.
///
/// RED-verified: with the CR/LF check removed from both the Lua boundary
/// (`vimcode.http.request`'s header loop in `plugin.rs`) and
/// `spawn_http_request`'s defensive re-check, this fails — the fixture
/// server observes a connection carrying both `X-Foo: bar` and a smuggled
/// `X-Injected: evilvalue` header.
#[test]
fn http_request_rejects_header_values_containing_crlf() {
    let mut e = engine_with_plugin(
        "",
        "http_header_crlf_1632",
        r#"
        vimcode.command("DoRequest", function(_)
            local ok, err = pcall(function()
                vimcode.http.request({
                    method = "GET",
                    url = "http://127.0.0.1:1/unreachable",
                    headers = { ["X-Foo"] = "bar\r\nX-Injected: evilvalue" },
                }, function(_resp) end)
            end)
            vimcode.message(tostring(ok) .. "|" .. tostring(err))
        end)
        "#,
    );
    exec(&mut e, "DoRequest");
    assert!(
        e.message.starts_with("false|"),
        "a header value containing CRLF must raise a Lua error, not silently \
         proceed to spawn curl; got {:?}",
        e.message
    );
    assert!(
        e.message.contains("CR") || e.message.contains("LF") || e.message.contains("\\r"),
        "the error should name what's wrong (CR/LF), not just fail silently: {:?}",
        e.message
    );
}

/// #1632 review: the request URL must reach `curl` as a value that can never
/// be parsed as another option — a bare positional argument lets a URL
/// string starting with `-` be interpreted by `curl` as a flag instead of a
/// URL. This proves it end-to-end with `-K<config>` (curl's "read more
/// options from this file" flag, which accepts its value attached with no
/// space): the config file below points curl at the *fixture server*, a
/// completely different destination than the (bogus) one the plugin
/// "asked" for. If the argument-injection bug is present, curl silently
/// loads that config and actually connects to the fixture server —
/// observable here as the fixture receiving a request at all. Fixed, `-K
/// <path>` is treated as a literal, unresolvable hostname and the fixture
/// never sees a connection.
///
/// RED-verified: with `--url` reverted back to a bare `command.arg(&spec.
/// url)`, this fails — the fixture server's `expected_requests: 1` deadline
/// is met, meaning curl really did load the injected config and connect to
/// it instead of failing to resolve the literal string as a hostname.
#[test]
fn http_request_url_that_looks_like_a_curl_flag_is_never_parsed_as_one() {
    let (req_tx, req_rx) = std::sync::mpsc::channel::<()>();
    let base_url = spawn_http_fixture_server(1, move |_, _, _, _| {
        let _ = req_tx.send(());
        (200, vec![], b"should never be reached".to_vec())
    });

    let config_path = std::env::temp_dir().join(format!(
        "vimcode_http_arg_injection_1632_{}.conf",
        std::process::id()
    ));
    std::fs::write(&config_path, format!("url = \"{base_url}/proof\"\n")).unwrap();

    // Attached form (`-K<path>`, no space) — exactly the shape the review
    // finding's own repro used (`"-o/tmp/x"`), and the shape that survives
    // being a single argv token (no shell is involved in `Command::arg`, so
    // an embedded space could never split into two argv entries anyway).
    let malicious_url = format!("-K{}", config_path.display());
    let mut e = engine_with_plugin(
        "",
        "http_url_injection_1632",
        &format!(
            r#"
            _G.done = false
            vimcode.command("DoRequest", function(_)
                vimcode.http.request({{
                    method = "GET", url = "{malicious_url}", timeout_ms = 2000,
                }}, function(resp)
                    _G.has_error = resp.error ~= nil and resp.error ~= ""
                    _G.done = true
                end)
            end)
            vimcode.command("ReadResult", function(_)
                vimcode.message(tostring(_G.done) .. "|" .. tostring(_G.has_error))
            end)
            "#
        ),
    );
    exec(&mut e, "DoRequest");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(10), |e| {
        exec(e, "ReadResult");
        e.message.starts_with("true|")
    });
    assert!(
        reached,
        "the request must resolve (as an error) rather than hang; last \
         message: {:?}",
        e.message
    );
    assert_eq!(
        e.message, "true|true",
        "a URL of the form `-K<path>` must be treated as a literal \
         (unresolvable) URL and surface as an error, never as `-K` loading \
         that file as a curl config"
    );
    assert!(
        req_rx.try_recv().is_err(),
        "the fixture server must never have received a connection — a \
         connection here means curl loaded the injected `-K` config and \
         followed the URL inside it instead of treating the malicious \
         string as a literal hostname"
    );

    let _ = std::fs::remove_file(&config_path);
}

#[test]
fn http_request_timeout_surfaces_as_an_error_not_a_response() {
    let base_url = spawn_http_fixture_server(1, |_, _, _, _| {
        std::thread::sleep(std::time::Duration::from_millis(800));
        (200, vec![], b"too-late".to_vec())
    });

    let mut e = engine_with_plugin(
        "",
        "http_timeout_1632",
        &format!(
            r#"
            _G.done = false
            vimcode.command("DoRequest", function(_)
                vimcode.http.request({{
                    method = "GET", url = "{base_url}/slow", timeout_ms = 100,
                }}, function(resp)
                    _G.has_error = resp.error ~= nil and resp.error ~= ""
                    _G.has_status = resp.status ~= nil
                    _G.done = true
                end)
            end)
            vimcode.command("ReadResult", function(_)
                vimcode.message(table.concat({{
                    tostring(_G.done), tostring(_G.has_error), tostring(_G.has_status),
                }}, "|"))
            end)
            "#
        ),
    );
    exec(&mut e, "DoRequest");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(10), |e| {
        exec(e, "ReadResult");
        e.message.starts_with("true|")
    });
    assert!(
        reached,
        "the timeout must resolve; last message: {:?}",
        e.message
    );
    assert_eq!(
        e.message, "true|true|false",
        "a 100ms timeout against a handler that sleeps 800ms must deliver \
         {{error=...}}, never a {{status=...}} response"
    );
}

/// `cancel()` must not merely drop `Engine`'s bookkeeping for the handle
/// (which on its own would already stop the callback from firing, since
/// `poll_plugin_http` only drains ids still present in the map) — it must
/// actually kill the `curl` child. This test's assertion has to be sensitive
/// to *that* distinction specifically, or reverting the `child.kill()` call
/// inside `Engine::plugin_api_http_cancel` back to a bookkeeping-only removal
/// would leave it green: a fixture server that delays its response and then
/// checks whether its delayed write to the peer socket still succeeds is
/// what tells the two apart — a killed `curl` process has already closed its
/// end of the connection, so the write fails; a merely-forgotten one is
/// still there to receive it.
///
/// RED-verified: with `Engine::plugin_api_http_cancel`'s `child.kill()` call
/// removed (leaving only the map removal), this fails — the fixture's
/// delayed write to the still-open socket succeeds.
#[test]
fn http_request_cancel_kills_the_curl_child_not_just_the_handle() {
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral loopback port");
    let addr = listener.local_addr().expect("resolve bound local_addr");
    let base_url = format!("http://{addr}");
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            // `curl` was killed before it ever connected — also proof the
            // process is gone, so this counts as "write did not succeed".
            let _ = tx.send(false);
            return;
        };
        let _ = read_http_fixture_request(&mut stream);
        // Give `cancel()` (called immediately after `Start`, well before
        // this elapses) plenty of time to have already killed the process.
        std::thread::sleep(std::time::Duration::from_millis(500));
        let body = b"too-late";
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
        let write_ok = stream.write_all(resp.as_bytes()).is_ok() && stream.write_all(body).is_ok();
        let _ = tx.send(write_ok);
    });

    let mut e = engine_with_plugin(
        "",
        "http_cancel_kill_1632",
        &format!(
            r#"
            _G.done = false
            vimcode.command("Start", function(_)
                _G.handle = vimcode.http.request({{
                    method = "GET", url = "{base_url}/never", timeout_ms = 30000,
                }}, function(_resp) _G.done = true end)
            end)
            vimcode.command("Cancel", function(_)
                _G.cancel_result = _G.handle.cancel()
            end)
            vimcode.command("Read", function(_)
                vimcode.message(tostring(_G.done) .. "|" .. tostring(_G.cancel_result))
            end)
            "#
        ),
    );
    exec(&mut e, "Start");
    exec(&mut e, "Cancel");
    exec(&mut e, "Read");
    assert_eq!(
        e.message, "false|true",
        "cancel() must report success and the callback must not have fired yet"
    );

    let write_ok = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or(false);
    assert!(
        !write_ok,
        "the curl child must actually be killed on cancel() — the fixture \
         server's delayed write to the connection succeeded, meaning the \
         process was still alive and reading on the other end"
    );

    // The callback still must never fire, however long we keep polling.
    for _ in 0..25 {
        e.poll_idle();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    exec(&mut e, "Read");
    assert_eq!(
        e.message, "false|true",
        "no response callback may fire for a cancelled request, ever"
    );
}

#[test]
fn http_response_callback_edits_buffer_via_immediate_api() {
    let base_url =
        spawn_http_fixture_server(1, |_, _, _, _| (200, vec![], b"FROM_SERVER_1632".to_vec()));

    let mut e = engine_with_plugin(
        "",
        "http_edit_buffer_1632",
        &format!(
            r#"
            vimcode.command("DoRequest", function(_)
                vimcode.http.request({{ method = "GET", url = "{base_url}/data" }}, function(resp)
                    local b = vimcode.buffer.create({{ scratch = true, name = "ZQ1632HTTP" }})
                    vimcode.buffer.set_lines(b, 0, -1, {{ resp.body }})
                    vimcode.window.set_buf(0, b)
                end)
            end)
            "#
        ),
    );
    exec(&mut e, "DoRequest");

    let reached = poll_until(&mut e, std::time::Duration::from_secs(10), |e| {
        e.buffer().to_string().contains("FROM_SERVER_1632")
    });
    assert!(
        reached,
        "the HTTP response callback must be able to use the immediate \
         vimcode.buffer API; buffer: {:?}",
        e.buffer().to_string()
    );
}

// ─── `vimcode.json` ──────────────────────────────────────────────────────────

#[test]
fn json_encode_decode_round_trips_and_distinguishes_empty_array_from_empty_object() {
    let mut e = engine_with_plugin(
        "",
        "json_roundtrip_1632",
        r#"
        vimcode.command("RunJson", function(_)
            local arr = vimcode.json.encode({1, 2, 3})
            local obj = vimcode.json.encode({foo = "bar"})
            local empty_arr = vimcode.json.encode({})
            local empty_obj = vimcode.json.encode(vimcode.json.empty_object)
            local decoded = vimcode.json.decode('{"x":1,"y":[1,2,3],"z":null}')
            local roundtrip_empty_obj = vimcode.json.encode(vimcode.json.decode("{}"))
            local roundtrip_empty_arr = vimcode.json.encode(vimcode.json.decode("[]"))
            local is_null = decoded.z == vimcode.json.null
            local ok, err = pcall(vimcode.json.decode, "not json")
            local err_mentions_decode = tostring(err):find("vimcode.json.decode", 1, true) ~= nil
            vimcode.message(table.concat({
                arr, obj, empty_arr, empty_obj,
                tostring(decoded.x), tostring(decoded.y[2]),
                roundtrip_empty_obj, roundtrip_empty_arr,
                tostring(is_null), tostring(ok), tostring(err_mentions_decode),
            }, "|"))
        end)
        "#,
    );
    exec(&mut e, "RunJson");
    assert_eq!(
        e.message, "[1,2,3]|{\"foo\":\"bar\"}|[]|{}|1|2|{}|[]|true|false|true",
        "arrays/objects must round-trip, an unmarked empty table must default \
         to '[]', vimcode.json.empty_object must force '{{}}', JSON null must \
         decode to the vimcode.json.null sentinel, and a decode error must be \
         pcall-catchable with a message naming the failing function"
    );
}

#[test]
fn json_encode_pretty_option_adds_newlines() {
    let mut e = engine_with_plugin(
        "",
        "json_pretty_1632",
        r#"
        vimcode.command("RunJson", function(_)
            local compact = vimcode.json.encode({a = 1})
            local pretty = vimcode.json.encode({a = 1}, { pretty = true })
            vimcode.message(tostring(compact:find("\n") == nil) .. "|"
                .. tostring(pretty:find("\n") ~= nil))
        end)
        "#,
    );
    exec(&mut e, "RunJson");
    assert_eq!(
        e.message, "true|true",
        "pretty=false (default) must be single-line; pretty=true must contain newlines"
    );
}

// ─── plugin-scoped storage ───────────────────────────────────────────────────

#[test]
fn storage_persists_across_plugin_reload() {
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_storage_persist_1632_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());

    let lua_code = r#"
        vimcode.command("Save", function(_)
            vimcode.storage.set("count", 42)
        end)
        vimcode.command("Load", function(_)
            vimcode.message(tostring(vimcode.storage.get("count")))
        end)
    "#;
    let mut e = engine_with_plugin("", "storage_persist_1632", lua_code);
    exec(&mut e, "Save");

    // Simulate a plugin reload: a fresh `PluginManager`, loaded from the
    // exact same plugin file `engine_with_plugin` wrote, installed in place
    // of the old one — same shape as #1624's `manager_swap_...` tests.
    let dir = plugin_fixture_dir("storage_persist_1632");
    let mut mgr2 = vimcode_core::core::plugin::PluginManager::new().expect("PluginManager::new");
    mgr2.load_plugins_dir(&dir, &[]);
    e.set_plugin_manager(mgr2);

    exec(&mut e, "Load");
    assert_eq!(
        e.message, "42",
        "storage must survive a plugin manager reload — it is disk-backed, \
         not held only in the (replaced) PluginManager"
    );
}

#[test]
fn storage_namespaces_by_plugin_so_plugins_cannot_read_each_others_data() {
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_storage_ns_1632_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());

    let dir =
        std::env::temp_dir().join(format!("vc_plugin_storage_ns_1632_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Both plugins use the *same* key name deliberately — the point of
    // namespacing is that this doesn't collide.
    std::fs::write(
        dir.join("ns_a.lua"),
        r#"
        vimcode.command("SetA", function(_) vimcode.storage.set("secret", "A-VALUE") end)
        vimcode.command("ReadA", function(_) vimcode.message(tostring(vimcode.storage.get("secret"))) end)
        "#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ns_b.lua"),
        r#"
        vimcode.command("SetB", function(_) vimcode.storage.set("secret", "B-VALUE") end)
        vimcode.command("ReadB", function(_) vimcode.message(tostring(vimcode.storage.get("secret"))) end)
        "#,
    )
    .unwrap();

    let mut e = engine_with("");
    let mut mgr = vimcode_core::core::plugin::PluginManager::new().expect("PluginManager::new");
    mgr.load_plugins_dir(&dir, &[]);
    e.set_plugin_manager(mgr);

    exec(&mut e, "SetA");
    exec(&mut e, "SetB");
    exec(&mut e, "ReadA");
    assert_eq!(
        e.message, "A-VALUE",
        "plugin A must read back its own value for a key plugin B also used"
    );
    exec(&mut e, "ReadB");
    assert_eq!(
        e.message, "B-VALUE",
        "plugin B must read back its own value, unclobbered by plugin A's \
         write to the same key name"
    );
}

/// #1632 review: `current_plugin_chunk_name` used to trust a Lua chunk's own
/// debug `source` unconditionally, and stock Lua's `load(chunk, chunkname)`
/// lets *any* caller set that `source` to an arbitrary string — so
/// `load(payload, "victim")()` let an attacker plugin impersonate another
/// plugin's storage namespace outright. The fix hardens the Lua-visible
/// `load` to always substitute the *caller's own* real identity for whatever
/// chunkname it requests. This test drives that from the attacker's actual
/// entry point (a registered command, exactly how a real malicious plugin
/// would trigger it) rather than calling any internal helper directly.
///
/// RED-verified: with the `load` hardening in `PluginManager::
/// setup_vimcode_api` reverted (restoring the stock global `load`), the
/// first assertion below fails — `AttackerReadViaSpoof` reads back
/// `"VICTIM-VALUE"` instead of `"nil"`.
#[test]
fn storage_load_cannot_spoof_another_plugins_chunk_name_to_steal_its_namespace() {
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_storage_spoof_1632_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());

    let dir = std::env::temp_dir().join(format!(
        "vc_plugin_storage_spoof_1632_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("victim.lua"),
        r#"
        vimcode.command("VictimSet", function(_) vimcode.storage.set("secret", "VICTIM-VALUE") end)
        vimcode.command("VictimRead", function(_) vimcode.message(tostring(vimcode.storage.get("secret"))) end)
        "#,
    )
    .unwrap();
    std::fs::write(
        dir.join("attacker.lua"),
        r#"
        vimcode.command("AttackerReadViaSpoof", function(_)
            -- Try to impersonate "victim" so `current_plugin_chunk_name`
            -- resolves to the victim's namespace instead of the attacker's.
            local forged = load("return vimcode.storage.get('secret')", "victim")
            vimcode.message(tostring(forged()))
        end)
        vimcode.command("AttackerWriteViaSpoof", function(_)
            local forged = load("vimcode.storage.set('secret', 'ATTACKER-CLOBBER')", "victim")
            forged()
            vimcode.message("wrote")
        end)
        "#,
    )
    .unwrap();

    let mut e = engine_with("");
    let mut mgr = vimcode_core::core::plugin::PluginManager::new().expect("PluginManager::new");
    mgr.load_plugins_dir(&dir, &[]);
    e.set_plugin_manager(mgr);

    exec(&mut e, "VictimSet");
    exec(&mut e, "AttackerReadViaSpoof");
    assert_eq!(
        e.message, "nil",
        "a spoofed load(payload, \"victim\") must NOT read the victim's \
         storage — it must land in the attacker's own (empty) namespace"
    );

    exec(&mut e, "AttackerWriteViaSpoof");
    exec(&mut e, "VictimRead");
    assert_eq!(
        e.message, "VICTIM-VALUE",
        "a spoofed load(payload, \"victim\") must NOT be able to clobber the \
         victim's storage either"
    );
}

#[test]
fn storage_workspace_scope_is_independent_of_global_scope() {
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_storage_scope_1632_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());

    let mut e = engine_with_plugin(
        "",
        "storage_scope_1632",
        r#"
        vimcode.command("SetGlobal", function(_) vimcode.storage.set("k", "global-value") end)
        vimcode.command("SetWorkspace", function(_)
            vimcode.storage.set("k", "workspace-value", { workspace = true })
        end)
        vimcode.command("ReadGlobal", function(_)
            vimcode.message(tostring(vimcode.storage.get("k")))
        end)
        vimcode.command("ReadWorkspace", function(_)
            vimcode.message(tostring(vimcode.storage.get("k", { workspace = true })))
        end)
        "#,
    );
    exec(&mut e, "SetGlobal");
    exec(&mut e, "SetWorkspace");
    exec(&mut e, "ReadGlobal");
    assert_eq!(
        e.message, "global-value",
        "the global scope must be unaffected by a workspace-scoped write"
    );
    exec(&mut e, "ReadWorkspace");
    assert_eq!(
        e.message, "workspace-value",
        "the workspace scope must be unaffected by the global write"
    );
}

#[test]
fn storage_delete_and_keys_reflect_current_contents() {
    let _lock = TOOL_ACQUIRE_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let data_home = std::env::temp_dir().join(format!(
        "vimcode_test_storage_keys_1632_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data_home);
    std::fs::create_dir_all(&data_home).unwrap();
    let _data_home_guard = EnvVarGuard::set("VIMCODE_TEST_DATA_HOME", data_home.as_os_str());

    let mut e = engine_with_plugin(
        "",
        "storage_keys_1632",
        r#"
        vimcode.command("Populate", function(_)
            vimcode.storage.set("a", 1)
            vimcode.storage.set("b", 2)
        end)
        vimcode.command("ReadKeys", function(_)
            vimcode.message(table.concat(vimcode.storage.keys(), ","))
        end)
        vimcode.command("DeleteA", function(_)
            _G.first_delete = vimcode.storage.delete("a")
            _G.second_delete = vimcode.storage.delete("a")
        end)
        vimcode.command("ReadDeleteResult", function(_)
            vimcode.message(tostring(_G.first_delete) .. "|" .. tostring(_G.second_delete)
                .. "|" .. tostring(vimcode.storage.get("a")))
        end)
        "#,
    );
    exec(&mut e, "Populate");
    exec(&mut e, "ReadKeys");
    assert_eq!(
        e.message, "a,b",
        "keys() must list every stored key, sorted"
    );

    exec(&mut e, "DeleteA");
    exec(&mut e, "ReadDeleteResult");
    assert_eq!(
        e.message, "true|false|nil",
        "delete() must report true the first time, false when the key is \
         already gone, and the value must read back as nil afterwards"
    );

    exec(&mut e, "ReadKeys");
    assert_eq!(
        e.message, "b",
        "the deleted key must no longer appear in keys()"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Native Extension API — Phase 5 (#1653): `vimcode.decor.*` — namespaces,
// extmarks that follow edits, highlight ranges, virtual text, signs, named
// highlight groups.
// ═══════════════════════════════════════════════════════════════════════════

/// A mark anchored to a line must move with it when a whole line is spliced
/// in above — the same rule real Vim's own marks follow (`O` splices the new
/// blank line in at the cursor's own line, so a mark *on* that line moves
/// too, not just marks strictly below it; see `test_mark_on_same_line_
/// shifts_when_o_inserts_above` in `new_vim_features.rs`).
///
/// RED against unfixed `develop`: there is no `vimcode.decor` table at all
/// there, so `SetupMark` errors out and the row never gets set up for `O` to
/// shift in the first place — confirmed by temporarily reverting the
/// `shift_marks_for_line_insert`/`shift_marks_for_line_delete` hooks into
/// `self.decor` (buffers.rs) with the API otherwise intact: the mark then
/// reads back `row=0` after `O`, not `row=1`.
#[test]
fn decor_mark_shifts_down_when_line_inserted_above_via_o() {
    let mut e = engine_with_plugin(
        "a\nb\nc\n",
        "decor_shift_insert",
        r#"
        local ns = nil
        local id = nil
        vimcode.command("SetupMark", function(_)
            ns = vimcode.decor.namespace("test_ns")
            id = vimcode.decor.set_mark(0, ns, { row = 0, col = 0, hl_group = "Test" })
            vimcode.message("ns=" .. ns .. " id=" .. id)
        end)
        vimcode.command("ReadMark", function(_)
            local m = vimcode.decor.get_mark(0, ns, id)
            if m == nil then
                vimcode.message("nil")
            else
                vimcode.message("row=" .. m.row .. " col=" .. m.col)
            end
        end)
        "#,
    );
    exec(&mut e, "SetupMark");
    assert!(
        e.message.starts_with("ns=") && e.message.contains("id="),
        "set_mark must return a mark id: {}",
        e.message
    );

    exec(&mut e, "ReadMark");
    assert_eq!(
        e.message, "row=0 col=0",
        "precondition: mark starts on row 0"
    );

    // `O` opens a new blank line above line 0, same as the vim-mark test.
    press(&mut e, 'O');
    type_chars(&mut e, "x");
    press_key(&mut e, "Escape");
    assert_eq!(
        get_lines(&e),
        vec![
            "x".to_string(),
            "a".to_string(),
            "b".to_string(),
            "c".to_string()
        ],
        "precondition: O must have spliced a new line in above row 0"
    );

    exec(&mut e, "ReadMark");
    assert_eq!(
        e.message, "row=1 col=0",
        "the mark must have shifted down with the line it was anchored to"
    );
}

/// A mark entirely inside a deleted range collapses (removed — `get_mark`
/// reads back `nil`); one below the deleted range shifts up with it.
#[test]
fn decor_mark_collapses_on_delete_and_shifts_for_marks_below() {
    let mut e = engine_with_plugin(
        "a\nb\nc\nd\n",
        "decor_shift_delete",
        r#"
        local ns = nil
        local deleted_id = nil
        local below_id = nil
        vimcode.command("SetupMarks", function(_)
            ns = vimcode.decor.namespace("test_ns")
            deleted_id = vimcode.decor.set_mark(0, ns, { row = 1, col = 0 })
            below_id = vimcode.decor.set_mark(0, ns, { row = 3, col = 0 })
        end)
        vimcode.command("ReadDeleted", function(_)
            local m = vimcode.decor.get_mark(0, ns, deleted_id)
            vimcode.message(m == nil and "nil" or ("row=" .. m.row))
        end)
        vimcode.command("ReadBelow", function(_)
            local m = vimcode.decor.get_mark(0, ns, below_id)
            vimcode.message(m == nil and "nil" or ("row=" .. m.row))
        end)
        "#,
    );
    exec(&mut e, "SetupMarks");

    // `dd` on line "b" (row 1) deletes exactly the line the first mark
    // anchors to.
    press(&mut e, 'j'); // row 1, "b"
    press(&mut e, 'd');
    press(&mut e, 'd');
    assert_eq!(
        get_lines(&e),
        vec!["a".to_string(), "c".to_string(), "d".to_string()],
        "precondition: dd must have removed row 1 (\"b\")"
    );

    exec(&mut e, "ReadDeleted");
    assert_eq!(
        e.message, "nil",
        "a mark whose entire range was deleted must collapse, not dangle on \
         whatever line slid into its old slot"
    );

    exec(&mut e, "ReadBelow");
    assert_eq!(
        e.message, "row=2",
        "a mark below the deleted line must shift up by the deleted line count"
    );
}

/// A mark's position survives a full `u`/`<C-r>` round trip — undo restores
/// it to where it was before the edit, redo re-applies the shift. Exercises
/// `BufferState::undo`/`redo`'s full-text-snapshot swap, which has no
/// per-edit line/count to shift by (unlike `O`/`dd`) — see
/// `Engine::shift_decor_across_undo_nav`'s doc comment.
#[test]
fn decor_mark_position_survives_undo_redo_round_trip() {
    let mut e = engine_with_plugin(
        "a\nb\nc\n",
        "decor_undo_redo",
        r#"
        local ns = nil
        local id = nil
        vimcode.command("SetupMark", function(_)
            ns = vimcode.decor.namespace("test_ns")
            id = vimcode.decor.set_mark(0, ns, { row = 2, col = 0 })
        end)
        vimcode.command("ReadMark", function(_)
            local m = vimcode.decor.get_mark(0, ns, id)
            vimcode.message(m == nil and "nil" or ("row=" .. m.row))
        end)
        "#,
    );
    exec(&mut e, "SetupMark");
    exec(&mut e, "ReadMark");
    assert_eq!(
        e.message, "row=2",
        "precondition: mark starts on row 2 (\"c\")"
    );

    // Insert a new line above everything, same as the insert test above.
    press(&mut e, 'O');
    type_chars(&mut e, "x");
    press_key(&mut e, "Escape");
    exec(&mut e, "ReadMark");
    assert_eq!(
        e.message, "row=3",
        "precondition: the insert must have shifted the mark down"
    );

    // `u`: undo the insert — the mark must come back to row 2.
    press(&mut e, 'u');
    assert_eq!(
        get_lines(&e),
        vec!["a", "b", "c"],
        "precondition: u must have undone the insert"
    );
    exec(&mut e, "ReadMark");
    assert_eq!(
        e.message, "row=2",
        "undo must relocate the mark back to its pre-insert row"
    );

    // `<C-r>`: redo the insert — the mark must shift down again.
    ctrl(&mut e, 'r');
    assert_eq!(
        get_lines(&e),
        vec!["x", "a", "b", "c"],
        "precondition: redo must have re-applied the insert"
    );
    exec(&mut e, "ReadMark");
    assert_eq!(
        e.message, "row=3",
        "redo must re-apply the shift the original insert caused"
    );
}

/// `vimcode.decor.clear(buf, ns, start, end)` removes only marks in `ns`
/// that touch the given row range, leaving marks in other namespaces (or
/// outside the range) alone.
#[test]
fn decor_clear_respects_namespace_and_row_range() {
    let mut e = engine_with_plugin(
        "a\nb\nc\nd\ne\n",
        "decor_clear",
        r#"
        local ns_a, ns_b = nil, nil
        local a1, a2, b1 = nil, nil, nil
        vimcode.command("Setup", function(_)
            ns_a = vimcode.decor.namespace("plugin_a")
            ns_b = vimcode.decor.namespace("plugin_b")
            a1 = vimcode.decor.set_mark(0, ns_a, { row = 0, col = 0 })
            a2 = vimcode.decor.set_mark(0, ns_a, { row = 4, col = 0 })
            b1 = vimcode.decor.set_mark(0, ns_b, { row = 0, col = 0 })
        end)
        vimcode.command("ReadAll", function(_)
            local function s(ns, id)
                local m = vimcode.decor.get_mark(0, ns, id)
                return m == nil and "nil" or "live"
            end
            vimcode.message("a1=" .. s(ns_a, a1) .. " a2=" .. s(ns_a, a2) .. " b1=" .. s(ns_b, b1))
        end)
        vimcode.command("ClearARange", function(_)
            vimcode.decor.clear(0, ns_a, 0, 1)
        end)
        "#,
    );
    exec(&mut e, "Setup");
    exec(&mut e, "ReadAll");
    assert_eq!(
        e.message, "a1=live a2=live b1=live",
        "precondition: all three marks exist"
    );

    // Clear namespace a's marks touching rows [0, 1) — only a1 qualifies.
    exec(&mut e, "ClearARange");
    exec(&mut e, "ReadAll");
    assert_eq!(
        e.message, "a1=nil a2=live b1=live",
        "clear must remove only the namespace-a mark inside the row range, \
         leaving namespace-a's out-of-range mark and namespace-b's mark alone \
         (namespace isolation)"
    );
}

/// Two different "plugins" (distinct namespaces) can set marks on the same
/// buffer without seeing or disturbing each other's: `namespace()` is
/// idempotent per name but distinct across names, and `get_mark`/`del_mark`
/// with the wrong namespace fail rather than reaching across.
#[test]
fn decor_namespace_isolation_between_plugins() {
    let mut e = engine_with_plugin(
        "a\nb\n",
        "decor_isolation",
        r#"
        vimcode.command("Run", function(_)
            local ns1 = vimcode.decor.namespace("plugin_one")
            local ns2 = vimcode.decor.namespace("plugin_two")
            local ns1_again = vimcode.decor.namespace("plugin_one")
            local id1 = vimcode.decor.set_mark(0, ns1, { row = 0, col = 0 })

            -- plugin_two cannot read plugin_one's mark by id.
            local cross_read = vimcode.decor.get_mark(0, ns2, id1)
            -- ...nor delete it.
            local cross_delete = vimcode.decor.del_mark(0, ns2, id1)
            local still_alive = vimcode.decor.get_mark(0, ns1, id1)

            vimcode.message(
                "distinct=" .. tostring(ns1 ~= ns2)
                .. " idempotent=" .. tostring(ns1 == ns1_again)
                .. " cross_read=" .. tostring(cross_read)
                .. " cross_delete=" .. tostring(cross_delete)
                .. " still_alive=" .. tostring(still_alive ~= nil)
            )
        end)
        "#,
    );
    exec(&mut e, "Run");
    assert_eq!(
        e.message,
        "distinct=true idempotent=true cross_read=nil cross_delete=false still_alive=true",
        "namespace() must mint distinct ids per name and be idempotent for \
         repeats of the same name; a different namespace must not be able to \
         read or delete another's mark"
    );
}

/// #1653 review: `get_mark`'s returned table must include `virt_text`/
/// `virt_text_pos` — before this fix it only returned `hl_group`/
/// `sign_text`/`sign_hl`, so a mark carrying virtual text couldn't be
/// read back at all, let alone round-tripped into another `set_mark`
/// call. RED against unfixed `develop`: `m.virt_text` reads back `nil`
/// and `#m.virt_text` errors (`attempt to get length of a nil value`)
/// instead of the chunk's text/hl_group below.
#[test]
fn decor_get_mark_round_trips_virt_text() {
    let mut e = engine_with_plugin(
        "a\n",
        "decor_get_mark_virt_text",
        r#"
        vimcode.command("Run", function(_)
            local ns = vimcode.decor.namespace("virt_text_roundtrip")
            local id = vimcode.decor.set_mark(0, ns, {
                row = 0, col = 0,
                virt_text = {{ text = "hint", hl_group = "Comment" }},
                virt_text_pos = "overlay",
            })
            local m = vimcode.decor.get_mark(0, ns, id)
            vimcode.message(
                "pos=" .. tostring(m.virt_text_pos)
                .. " n=" .. tostring(#m.virt_text)
                .. " text=" .. tostring(m.virt_text[1].text)
                .. " hl=" .. tostring(m.virt_text[1].hl_group)
            )
        end)
        "#,
    );
    exec(&mut e, "Run");
    assert_eq!(
        e.message, "pos=overlay n=1 text=hint hl=Comment",
        "get_mark must return virt_text/virt_text_pos in the same shape \
         set_mark accepts them in"
    );
}

/// `vimcode.buf.annotate_line`/`clear_annotations` (the legacy, per-line
/// virtual-text mechanism #1653 explicitly leaves untouched) must keep
/// behaving exactly as before now that `vimcode.decor.*` exists alongside
/// it — same `engine.line_annotations` map, same 1-indexed line argument,
/// same full-clear semantics.
#[test]
fn annotate_line_behaviour_is_unchanged_by_decor_api() {
    let mut e = engine_with_plugin(
        "one\ntwo\nthree\n",
        "annotate_unchanged",
        r#"
        vimcode.command("Annotate", function(_)
            vimcode.buf.annotate_line(2, "blame: Jane")
        end)
        vimcode.command("ClearAll", function(_)
            vimcode.buf.clear_annotations()
        end)
        "#,
    );
    exec(&mut e, "Annotate");
    assert_eq!(
        e.line_annotations.get(&1).map(String::as_str),
        Some("blame: Jane"),
        "annotate_line(2, ...) must still set the 0-indexed line_annotations \
         entry at index 1"
    );

    exec(&mut e, "ClearAll");
    assert!(
        e.line_annotations.is_empty(),
        "clear_annotations must still wipe every entry"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Native Extension API — Phase 6 (#1654): `vimcode.syntax.*` (read-only
// tree-sitter node/query access over the engine's own highlighting parse),
// `vimcode.undo.*` (undo-tree read + jump), `vimcode.diagnostics.*` (LSP
// diagnostics read + `DiagnosticChanged`).
// ═══════════════════════════════════════════════════════════════════════════

/// Install a tree-sitter parser for `lang` on the active buffer and parse
/// its current text — the same two steps `BufferState::with_file` runs on a
/// real file open, minus the filename-sniffing (these fixtures pick the
/// language explicitly, so a throwaway temp path doesn't need a real
/// `.rs`/`.lua` extension).
fn setup_syntax(e: &mut vimcode_core::Engine, lang: vimcode_core::core::syntax::SyntaxLanguage) {
    e.active_buffer_state_mut().syntax =
        Some(vimcode_core::core::syntax::Syntax::new_for_language(lang));
    e.active_buffer_state_mut().update_syntax();
}

/// RED against unfixed `develop`: there is no `vimcode.syntax` table there,
/// so `ReadNode`'s callback errors on its first line and `e.message` is
/// never set to the `type=...` string this asserts on.
#[test]
fn syntax_node_at_finds_function_identifier_in_rust_fixture() {
    use vimcode_core::core::syntax::SyntaxLanguage;
    let mut e = engine_with_plugin(
        "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        "syntax_node_at_rust",
        r#"
        vimcode.command("ReadNode", function(_)
            local node = vimcode.syntax.node_at(0, 0, 3)
            vimcode.message(
                "type=" .. node.type
                .. " lang=" .. node.language
                .. " parent=" .. tostring(node.parent and node.parent.type)
            )
        end)
        "#,
    );
    setup_syntax(&mut e, SyntaxLanguage::Rust);

    exec(&mut e, "ReadNode");
    assert_eq!(
        e.message, "type=identifier lang=rust parent=function_item",
        "node_at(0, 0, 3) (byte col 3 = the 'a' of \"add\", right after \"fn \") \
         must resolve the function name identifier, with its immediate \
         parent being the enclosing function_item: {}",
        e.message
    );
}

/// Same shape as the Rust fixture above, for Lua — a different grammar, a
/// different node-kind vocabulary (`function_declaration` instead of
/// `function_item`), proving `node_at` isn't accidentally Rust-specific.
#[test]
fn syntax_node_at_finds_function_identifier_in_lua_fixture() {
    use vimcode_core::core::syntax::SyntaxLanguage;
    let mut e = engine_with_plugin(
        "function add(a, b)\n  return a + b\nend\n",
        "syntax_node_at_lua",
        r#"
        vimcode.command("ReadNode", function(_)
            local node = vimcode.syntax.node_at(0, 0, 9)
            vimcode.message(
                "type=" .. node.type
                .. " lang=" .. node.language
                .. " parent=" .. tostring(node.parent and node.parent.type)
            )
        end)
        "#,
    );
    setup_syntax(&mut e, SyntaxLanguage::Lua);

    exec(&mut e, "ReadNode");
    assert_eq!(
        e.message, "type=identifier lang=lua parent=function_declaration",
        "node_at(0, 0, 9) (byte col 9 = the 'a' of \"add\", right after \
         \"function \") must resolve the function name identifier, with its \
         immediate parent being the enclosing function_declaration: {}",
        e.message
    );
}

/// A buffer with no tree-sitter grammar (no `syntax` installed at all) must
/// error clearly from `node_at`, not panic or silently return `nil`.
///
/// RED against unfixed `develop`: no `vimcode.syntax` table exists there at
/// all, so this would already error — but with a *different* message
/// ("attempt to index a nil value"), which would pass this test's
/// `ok=false` half without ever exercising the "buffer has no parser"
/// error path `plugin_api_syntax_node_at` is actually responsible for.
/// Confirmed by temporarily making `Engine::plugin_api_syntax_node_at`
/// return `Ok` with a dummy node regardless of `state.syntax`: the test then
/// fails on the `err` half, not the `ok` half.
#[test]
fn syntax_node_at_errors_clearly_with_no_parser() {
    let mut e = engine_with_plugin(
        "plain text, no language set\n",
        "syntax_node_at_no_parser",
        r#"
        vimcode.command("ReadNode", function(_)
            local ok, err = pcall(function()
                return vimcode.syntax.node_at(0, 0, 0)
            end)
            vimcode.message("ok=" .. tostring(ok) .. " err=" .. tostring(err))
        end)
        "#,
    );
    exec(&mut e, "ReadNode");
    assert!(
        e.message.contains("ok=false") && e.message.contains("no parser"),
        "node_at on a buffer with no tree-sitter parser must error clearly: {}",
        e.message
    );
}

/// `vimcode.syntax.query` compiles and runs an ad hoc query against the
/// buffer's language, independent of whatever built-in highlight query
/// `Syntax` was constructed with.
#[test]
fn syntax_query_finds_function_name_captures_in_rust_fixture() {
    use vimcode_core::core::syntax::SyntaxLanguage;
    let mut e = engine_with_plugin(
        "fn add() {}\nfn sub() {}\n",
        "syntax_query_rust",
        r#"
        vimcode.command("RunQuery", function(_)
            local caps = vimcode.syntax.query(0, "(function_item name: (identifier) @function)")
            local names = {}
            for _, c in ipairs(caps) do
                table.insert(names, c.name .. ":" .. c.type .. ":" .. c.range.start_row)
            end
            vimcode.message(table.concat(names, ","))
        end)
        "#,
    );
    setup_syntax(&mut e, SyntaxLanguage::Rust);

    exec(&mut e, "RunQuery");
    assert_eq!(
        e.message, "function:identifier:0,function:identifier:1",
        "the query must capture both function names, one per row: {}",
        e.message
    );
}

/// `vimcode.undo.tree` shape after two edits, an undo, and a third edit
/// that starts a new branch — the exact scenario the issue's acceptance
/// bar names ("undo tree shape after edits + undo + new branch").
///
/// RED against unfixed `develop`: there is no `vimcode.undo` table there,
/// so `ReadTree`'s callback errors on its first line.
#[test]
fn undo_tree_shows_shape_after_edits_undo_and_new_branch() {
    let mut e = engine_with_plugin(
        "a\n",
        "undo_tree_shape",
        r#"
        vimcode.command("ReadTree", function(_)
            local nodes = vimcode.undo.tree(0)
            local parts = {}
            for _, n in ipairs(nodes) do
                table.insert(parts, n.seq .. ":" .. tostring(n.parent) .. ":" .. tostring(n.current))
            end
            vimcode.message(table.concat(parts, ","))
        end)
        "#,
    );

    // seq 1: "a" -> "ab"
    press(&mut e, 'A');
    type_chars(&mut e, "b");
    press_key(&mut e, "Escape");
    // seq 2: "ab" -> "abc"
    press(&mut e, 'A');
    type_chars(&mut e, "c");
    press_key(&mut e, "Escape");
    // Undo back to seq 1 ("ab"), abandoning seq 2's branch (still live, not
    // pruned — just no longer on the active path).
    e.undo();
    // seq 3: a *new* branch off seq 1: "ab" -> "abd".
    press(&mut e, 'A');
    type_chars(&mut e, "d");
    press_key(&mut e, "Escape");

    exec(&mut e, "ReadTree");
    assert_eq!(
        e.message, "0:nil:false,1:0:false,2:1:false,3:1:true",
        "must list the root, the live-but-abandoned seq-2 branch, and the \
         new seq-3 branch (off seq-1, now current): {}",
        e.message
    );
}

/// `vimcode.undo.jump(buf, seq)` — the one mutation P6 ships — restores the
/// exact text of the named `seq`, going through the existing undo tree
/// rather than a bespoke buffer swap.
///
/// RED against unfixed `develop`: there is no `vimcode.undo` table there,
/// so `JumpTo`'s callback errors and the buffer keeps reading "abd", not
/// "ab".
#[test]
fn undo_jump_restores_text_of_a_named_seq() {
    let mut e = engine_with_plugin(
        "a\n",
        "undo_jump",
        r#"
        vimcode.command("JumpTo1", function(_)
            local ok = vimcode.undo.jump(0, 1)
            vimcode.message("jumped=" .. tostring(ok))
        end)
        "#,
    );

    press(&mut e, 'A');
    type_chars(&mut e, "b");
    press_key(&mut e, "Escape");
    press(&mut e, 'A');
    type_chars(&mut e, "c");
    press_key(&mut e, "Escape");
    assert_eq!(get_lines(&e), vec!["abc".to_string()], "precondition");

    exec(&mut e, "JumpTo1");
    assert_eq!(e.message, "jumped=true");
    assert_eq!(
        get_lines(&e),
        vec!["ab".to_string()],
        "jump(0, 1) must restore seq 1's text (\"ab\"), not stay on seq 2"
    );
}

/// `vimcode.diagnostics.get()` (no `buf` argument — defaults to the active
/// buffer) reads back a diagnostic set installed via
/// `Engine::set_diagnostics_for_path` — the same seam `poll_lsp`'s
/// `LspEvent::Diagnostics` handling writes through in production, so this
/// is "diagnostics read from an injected diagnostic set" without needing a
/// live LSP server.
///
/// RED against unfixed `develop`: there is no `vimcode.diagnostics` table
/// there, so `ReadDiags`'s callback errors and `e.message` never gets set.
#[test]
fn diagnostics_get_reads_an_injected_diagnostic_set() {
    use vimcode_core::core::lsp::{Diagnostic, DiagnosticSeverity, LspPosition, LspRange};
    let mut e = engine_with_plugin(
        "line one\nline two\n",
        "diagnostics_get",
        r#"
        vimcode.command("ReadDiags", function(_)
            local diags = vimcode.diagnostics.get()
            local d = diags[1]
            vimcode.message(
                "n=" .. #diags
                .. " sev=" .. d.severity
                .. " msg=" .. d.message
                .. " row=" .. d.range.start_row
                .. " col=" .. d.range.start_col
            )
        end)
        "#,
    );
    let path = std::path::PathBuf::from("/tmp/vc_diag_test_1654.rs");
    e.active_buffer_state_mut().file_path = Some(path.clone());
    e.set_diagnostics_for_path(
        path,
        vec![Diagnostic {
            range: LspRange {
                start: LspPosition {
                    line: 0,
                    character: 2,
                },
                end: LspPosition {
                    line: 0,
                    character: 6,
                },
            },
            severity: DiagnosticSeverity::Error,
            message: "unexpected token".to_string(),
            source: Some("rustc".to_string()),
            code: Some("E0308".to_string()),
        }],
    );

    exec(&mut e, "ReadDiags");
    assert_eq!(
        e.message, "n=1 sev=error msg=unexpected token row=0 col=2",
        "vimcode.diagnostics.get() must read back the injected diagnostic: {}",
        e.message
    );
}

/// `DiagnosticChanged` fires with the changed path whenever diagnostics are
/// installed for it, via the existing generic `vimcode.on` registration —
/// no separate event-wiring API needed.
///
/// RED against unfixed `develop`: `Engine::set_diagnostics_for_path`
/// doesn't exist there (only a direct `lsp_diagnostics.insert`), so there is
/// no event to fire and `e.message` stays whatever it was before.
#[test]
fn diagnostic_changed_event_fires_when_diagnostics_are_installed() {
    use vimcode_core::core::lsp::Diagnostic;
    let mut e = engine_with_plugin(
        "line one\n",
        "diagnostic_changed",
        r#"
        vimcode.on("DiagnosticChanged", function(path)
            vimcode.message("changed:" .. path)
        end)
        "#,
    );
    let path = std::path::PathBuf::from("/tmp/vc_diag_changed_1654.rs");
    e.set_diagnostics_for_path(path.clone(), Vec::<Diagnostic>::new());

    assert_eq!(
        e.message,
        format!("changed:{}", path.to_string_lossy()),
        "DiagnosticChanged must fire with the path whose diagnostics changed"
    );
}
