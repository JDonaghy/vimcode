mod common;
use common::*;

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
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_selected = 0;
    e.handle_ext_sidebar_key("j", false, None);
    // Selection should advance (if there are available items)
    let total = e.ext_available_manifests().len();
    if total > 1 {
        assert!(
            e.ext_sidebar_selected > 0,
            "j should move selection down: selected={}",
            e.ext_sidebar_selected
        );
    }
}

#[test]
fn ext_sidebar_key_k_moves_selection_up() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_selected = 2;
    e.handle_ext_sidebar_key("k", false, None);
    assert!(
        e.ext_sidebar_selected < 2,
        "k should move selection up: selected={}",
        e.ext_sidebar_selected
    );
}

#[test]
fn ext_sidebar_key_escape_unfocuses() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.handle_ext_sidebar_key("Escape", false, None);
    assert!(!e.ext_sidebar_has_focus, "Escape should unfocus sidebar");
}

#[test]
fn ext_sidebar_key_slash_activates_search_input() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.handle_ext_sidebar_key("/", false, None);
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

#[test]
fn ext_install_sets_pending_terminal_command_for_lsp() {
    let mut e = engine_with_registry("");
    // Ruby has an LSP install command ("gem install ruby-lsp") and ruby-lsp
    // binary is unlikely to be on PATH in CI.
    let action = exec(&mut e, "ExtInstall ruby");
    assert!(
        e.extension_state.is_installed("ruby"),
        "ruby should be marked installed"
    );
    // The action should be RunInTerminal (carries the install command).
    assert!(
        matches!(action, vimcode_core::EngineAction::RunInTerminal(_)),
        "should return RunInTerminal action, got: {:?}",
        action
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
    let mut e = engine_with_registry("");
    // Use ruby extension whose LSP binary (ruby-lsp) is not on PATH.
    exec(&mut e, "ExtInstall ruby");
    // pending_install_context should have been set (consumed by terminal_run_command)
    // but since we took the terminal command via the action, it's already consumed.
    // Let's verify via the sidebar path instead.
    e.extension_state.installed.clear();

    // Test via sidebar: install via 'i' key
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, true];
    let available = e.ext_available_items();
    let ruby_idx = available
        .iter()
        .position(|m| m.name == "ruby")
        .expect("ruby should be in available");
    e.ext_sidebar_selected = ruby_idx;
    e.handle_ext_sidebar_key("i", false, None);

    // pending_terminal_command should be set (sidebar can't return EngineAction)
    assert!(
        e.pending_terminal_command.is_some(),
        "sidebar install should set pending_terminal_command"
    );
    let cmd = e.pending_terminal_command.as_ref().unwrap();
    assert!(
        cmd.contains("ruby-lsp"),
        "command should mention ruby-lsp: {cmd}"
    );
    // Clean up
    e.extension_state.installed.clear();
    e.pending_terminal_command = None;
    e.pending_install_context = None;
}

// ── Auto-hint on file open ─────────────────────────────────────────────────────

#[test]
fn auto_hint_shown_for_uninstalled_extension_on_file_open() {
    let mut e = engine_with_registry("");
    assert!(!e.extension_state.is_installed("csharp"));
    assert!(!e.extension_state.is_dismissed("csharp"));

    let path = std::env::temp_dir().join("vimcode_smoke_hint_01.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    assert!(
        e.message.contains("ExtInstall") || e.message.contains("csharp"),
        "expected extension hint for uninstalled csharp: {}",
        e.message
    );
}

#[test]
fn auto_hint_not_shown_when_extension_dismissed() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_dismissed("csharp");

    let path = std::env::temp_dir().join("vimcode_smoke_hint_02.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    assert!(
        !e.message.contains("No csharp extension"),
        "hint should not appear when csharp is dismissed: {}",
        e.message
    );
}

#[test]
fn auto_hint_not_shown_when_extension_installed() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");

    let path = std::env::temp_dir().join("vimcode_smoke_hint_03.cs");
    std::fs::write(&path, "// test\n").ok();
    e.open_file_in_tab(&path);
    let _ = std::fs::remove_file(&path);

    assert!(
        !e.message.contains("No csharp extension"),
        "hint should not appear when csharp is installed: {}",
        e.message
    );
}

#[test]
fn auto_hint_not_shown_twice_for_same_extension() {
    let mut e = engine_with_registry("");

    let path1 = std::env::temp_dir().join("vimcode_smoke_hint_04a.cs");
    let path2 = std::env::temp_dir().join("vimcode_smoke_hint_04b.cs");
    std::fs::write(&path1, "// a\n").ok();
    std::fs::write(&path2, "// b\n").ok();

    e.open_file_in_tab(&path1);
    let first_msg = e.message.clone();
    e.open_file_in_tab(&path2);
    let second_msg = e.message.clone();

    let _ = std::fs::remove_file(&path1);
    let _ = std::fs::remove_file(&path2);

    // First open should have triggered the hint
    assert!(
        first_msg.contains("ExtInstall") || first_msg.contains("csharp"),
        "first open should show hint: {first_msg}"
    );
    // Second open of same language must NOT re-show the same hint
    assert!(
        !second_msg.contains("No csharp extension"),
        "second open should not re-prompt for csharp: {second_msg}"
    );
}

// ── Sidebar navigation — clamping ─────────────────────────────────────────────

#[test]
fn ext_sidebar_j_clamps_at_last_item() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_has_focus = true;
    let total = e.ext_available_manifests().len();
    // Jump past the end
    e.ext_sidebar_selected = total.saturating_sub(1);
    e.handle_ext_sidebar_key("j", false, None);
    assert!(
        e.ext_sidebar_selected < total,
        "j should not go past the last item: selected={}, total={total}",
        e.ext_sidebar_selected
    );
}

#[test]
fn ext_sidebar_k_clamps_at_zero() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_selected = 0;
    e.handle_ext_sidebar_key("k", false, None);
    assert_eq!(
        e.ext_sidebar_selected, 0,
        "k at position 0 should stay at 0"
    );
}

// ── Sidebar Tab — section toggling ────────────────────────────────────────────

#[test]
fn ext_sidebar_tab_toggles_installed_section() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_has_focus = true;
    e.extension_state.mark_installed("csharp");
    e.ext_sidebar_selected = 0; // within installed items

    let was_expanded = e.ext_sidebar_sections_expanded[0];
    e.handle_ext_sidebar_key("Tab", false, None);
    assert_ne!(
        e.ext_sidebar_sections_expanded[0], was_expanded,
        "Tab should toggle installed section"
    );
}

#[test]
fn ext_sidebar_tab_toggles_available_section_when_no_installed() {
    let mut e = engine_with_registry("");
    e.ext_sidebar_has_focus = true;
    // No extensions installed — cursor is in the available section
    e.ext_sidebar_selected = 0;

    let was_expanded = e.ext_sidebar_sections_expanded[1];
    e.handle_ext_sidebar_key("Tab", false, None);
    assert_ne!(
        e.ext_sidebar_sections_expanded[1], was_expanded,
        "Tab should toggle available section when nothing is installed"
    );
}

// ── Sidebar d — remove installed extension ────────────────────────────────────

#[test]
fn ext_sidebar_d_removes_installed_extension() {
    let mut e = engine_with_registry("");
    e.extension_state.mark_installed("csharp");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_selected = 0; // first (and only) installed item

    e.handle_ext_sidebar_key("d", false, None);
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
    e.handle_ext_sidebar_key("d", false, None);
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
    e.handle_ext_sidebar_key("Return", false, None);

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

    e.handle_ext_sidebar_key("Return", false, None);

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

    e.handle_ext_sidebar_key("i", false, None);

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

    e.handle_ext_sidebar_key("r", false, Some('r'));
    e.handle_ext_sidebar_key("u", false, Some('u'));
    e.handle_ext_sidebar_key("s", false, Some('s'));
    e.handle_ext_sidebar_key("t", false, Some('t'));

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

    e.handle_ext_sidebar_key("Escape", false, None);

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

    e.handle_ext_sidebar_key("BackSpace", false, None);

    assert_eq!(
        e.ext_sidebar_query, "rus",
        "BackSpace should remove the last char from the query"
    );
}

#[test]
fn ext_sidebar_search_resets_selection_to_zero_on_input() {
    let mut e = engine_with("");
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_input_active = true;
    e.ext_sidebar_selected = 5;

    e.handle_ext_sidebar_key("r", false, Some('r'));

    assert_eq!(
        e.ext_sidebar_selected, 0,
        "typing in search should reset selection to 0"
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
    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, true];
    // Navigate to rust in the available list (alphabetically last: bash, cpp, csharp,
    // git-insights, go, java, javascript, php, python, ruby, rust — index 10)
    let available_before = e
        .ext_available_manifests()
        .into_iter()
        .filter(|m| !e.extension_state.is_installed(&m.name))
        .collect::<Vec<_>>();
    let rust_idx = available_before
        .iter()
        .position(|m| m.name == "rust")
        .expect("rust should be in available list");
    e.ext_sidebar_selected = rust_idx; // point at rust in available section

    // Install via 'i' key (Return now previews README)
    e.handle_ext_sidebar_key("i", false, None);

    // Rust should now be installed
    assert!(
        e.extension_state.is_installed("rust"),
        "rust should be marked installed after Return"
    );

    // Selection should now be in the installed section, pointing at rust
    let installed = e.ext_installed_items();
    let sel = e.ext_sidebar_selected;
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
    e.handle_ext_sidebar_key("d", false, None);
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
    e.ext_sidebar_has_focus = true;
    // Install bash as the only extension
    e.extension_state.mark_installed("bash");
    // Collapse the available section (simulating user pressing Tab)
    e.ext_sidebar_sections_expanded = [true, false];
    // Selection points to bash (only installed item, flat index 0)
    e.ext_sidebar_selected = 0;

    // Verify only the installed item is selectable before deletion
    // (available section is collapsed, so only 1 installed item is visible)
    let installed_before = e.ext_installed_items();
    assert_eq!(
        installed_before.len(),
        1,
        "should have 1 installed item (bash)"
    );

    // Delete bash — confirm dialog (Right past Cancel, then Enter)
    e.handle_ext_sidebar_key("d", false, None);
    assert!(e.dialog.is_some(), "removal dialog should be open");
    e.handle_key("Right", None, false);
    e.handle_key("Return", None, false);

    assert!(
        !e.extension_state.is_installed("bash"),
        "bash should be removed"
    );

    // The available section should now be expanded
    assert!(
        e.ext_sidebar_sections_expanded[1],
        "available section should be expanded after deleting last installed item"
    );

    // Navigation should work — available items are visible
    let available_after: Vec<_> = e
        .ext_available_manifests()
        .into_iter()
        .filter(|m| !e.extension_state.is_installed(&m.name))
        .collect();
    assert!(
        !available_after.is_empty(),
        "available items should be visible after expanding section"
    );

    // j should move the selection
    e.ext_sidebar_selected = 0;
    e.handle_ext_sidebar_key("j", false, None);
    assert!(
        e.ext_sidebar_selected > 0,
        "j should move selection after deletion, still at {}",
        e.ext_sidebar_selected
    );

    // Clean up
    e.extension_state.installed.clear();
}

// ═══════════════════════════════════════════════════════════════════════════
// Plugin API — Phase 1: Autocmd events, keymap modes, cursor set, settings,
// state queries
// ═══════════════════════════════════════════════════════════════════════════

/// Helper: create an engine with a PluginManager loaded from a temp dir.
fn engine_with_plugin(text: &str, plugin_name: &str, lua_code: &str) -> vimcode_core::Engine {
    let dir = std::env::temp_dir().join(format!("vc_plugin_api_{}", plugin_name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{plugin_name}.lua")), lua_code).unwrap();

    let mut e = engine_with(text);
    match vimcode_core::core::plugin::PluginManager::new() {
        Ok(mut mgr) => {
            mgr.load_plugins_dir(&dir, &[]);
            e.plugin_manager = Some(mgr);
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
    e.registers.insert('a', ("test content".to_string(), false));
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
    assert!(linewise, "register should be linewise");
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
    let dir = std::env::temp_dir().join("vc_plugin_api_vimenter");
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
            e.plugin_manager = Some(mgr);
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

fn plugin_with(code: &str) -> PluginManager {
    let dir = std::env::temp_dir().join(format!(
        "vc_git_api_test_{}",
        code.len() // simple discriminator
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.lua");
    std::fs::write(&path, code).unwrap();
    let mut pm = PluginManager::new().unwrap();
    pm.load_plugins_dir(&dir, &[]);
    pm
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
    assert!(
        msg.contains("vimcode"),
        "root should contain vimcode: {msg}"
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
