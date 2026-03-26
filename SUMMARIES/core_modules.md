# Core Modules (src/core/)

## lsp.rs — 2,486 lines
LSP protocol transport and single-server client.
### Types
- `LspServer` — manages a single LSP server process (stdin/stdout/stderr, reader thread)
- `LspEvent` — enum of all LSP response types (Completion, Definition, Hover, Diagnostics, SemanticTokens, etc.)
- `Diagnostic` / `DiagnosticSeverity` — diagnostic data
- `CodeAction` / `CompletionItem` / `Location` / `LspRange` / `LspPosition` — LSP data types
- `WorkspaceEdit` / `FileEdit` / `FormattingEdit` — edit application types
- `SemanticToken` / `SemanticTokensLegend` — semantic token data
- `SignatureHelpData` — function signature info
- `LspServerConfig` — server command + args + language mappings
- `MasonPackageInfo` — Mason package metadata
### Key Functions
- `LspServer::start(config)` — spawn LSP process, send initialize, start reader thread
- `did_open/did_change/did_save/did_close` — document sync notifications
- `request_completion/definition/hover/references/implementation/rename/code_action/formatting/semantic_tokens_full` — LSP requests
- `decode_semantic_tokens(raw, legend)` — delta-decode semantic token array
- `path_to_uri/uri_to_path` — file path ↔ URI conversion
- `language_id_from_path(path)` — file extension to language ID

## lsp_manager.rs — 982 lines
Multi-server LSP coordinator. Manages server lifecycle per language.
### Types
- `LspManager` — holds active servers, registry, extension manifests
### Key Functions
- `LspManager::new(root, user_servers)` — create with workspace root
- `ensure_server_for_language(lang_id)` — start server if needed
- `poll_events()` — collect events from all active servers
- `notify_did_open/change/save/close(path)` — route notifications to correct server
- `request_completion/definition/hover/references/...` — route requests by file path
- `restart_server_for_language(lang_id)` / `stop_server_for_language(lang_id)`
- `server_info(current_lang)` — `:LspInfo` output
- `default_server_registry()` — built-in server configs

## dap.rs — 707 lines
DAP protocol transport (Debug Adapter Protocol).
### Types
- `DapClient` — manages a single DAP adapter process
- `DapEvent` — enum of DAP events (Stopped, Output, Terminated, etc.)
- `DapVariable` — variable tree node with children
### Key Functions
- `DapClient::launch(cmd, args)` — spawn adapter, send initialize
- `set_breakpoints(path, breakpoints)` — configure breakpoints
- `continue_/next/step_in/step_out/pause` — execution control
- `evaluate(expr, frame_id)` — evaluate expression in debug context

## dap_manager.rs — 1,423 lines
DAP adapter registry, launch.json parsing, tasks.json support.
### Types
- `DapManager` — adapter registry and launch config management
- `LaunchConfig` — parsed launch.json entry
- `TaskDefinition` — parsed tasks.json entry
### Key Functions
- `DapManager::new()` — create with 6 built-in adapters
- `start_session(config)` — launch debug session
- `parse_launch_json(path)` — parse `.vimcode/launch.json`
- `generate_launch_json(lang)` — auto-generate launch config

## git.rs — 2,538 lines
Git subprocess integration. All git operations via `Command` spawning.
### Types
- `GitLineStatus` — Added/Modified/Deleted/Untracked per line
- `StatusKind` / `FileStatus` / `WorktreeEntry` — git status data
- `Hunk` / `DiffHunkInfo` — diff hunk data
- `BlameInfo` — git blame line info
- `GitLogEntry` — git log entry
### Key Functions
- `find_repo_root(path)` — walk up to find `.git`
- `current_branch(dir)` — active branch name
- `status(dir)` — parsed `git status --porcelain`
- `file_diff_text(path)` — raw diff output
- `stage_file(path)` / `stage_all(dir)` — git add
- `stage_hunk(dir, header, hunk)` / `revert_hunk(dir, header, hunk)` — hunk-level operations
- `commit(dir, message)` — git commit
- `push/pull/fetch(dir)` — remote operations (+ passphrase variants)
- `blame_line(repo, file, line)` — single-line blame
- `log_file(repo, file, limit)` — file history

## plugin.rs — 1,915 lines
Lua 5.4 plugin manager (mlua 0.9).
### Types
- `PluginManager` — Lua VM, loaded plugins, registered commands/keymaps/hooks
- `LoadedPlugin` — plugin name + path
- `PluginCallContext` — input/output struct for plugin dispatch (avoids double-borrow)
### Key Functions
- `PluginManager::new()` — create Lua VM, install `vimcode.*` API
- `load_plugins_dir(dir, disabled)` — scan and load plugins
- `call_command(name, args, ctx)` / `call_event(event, ctx)` / `call_keymap(mode, key, ctx)` — dispatch
- `setup_vimcode_api(lua)` — register `vimcode.*` Lua globals

## buffer_manager.rs — 908 lines
Buffer storage and management.
### Types
- `BufferManager` — `HashMap<BufferId, BufferState>` wrapper
- `BufferState` — buffer content (Ropey rope), file path, dirty flag, syntax tree, undo/redo stacks, git diff, semantic tokens, diff label
- `Buffer` — Ropey rope wrapper with line/char accessors
### Key Functions
- `BufferManager::create(path)` / `get(id)` / `get_mut(id)` / `remove(id)`
- `BufferState::from_text(text)` / `from_file(path)` — buffer creation

## syntax.rs — 1,522 lines
Tree-sitter syntax highlighting for 20 languages.
### Types
- `SyntaxHighlighter` — tree-sitter parser + tree per buffer
- `SyntaxLanguage` — enum of 20 supported languages
### Key Functions
- `SyntaxHighlighter::new(language)` — create parser for language
- `parse(text)` / `edit_and_reparse(text, edit)` — incremental parsing
- `highlight_line(line, text)` — get syntax spans for a line
- `language_for_extension(ext)` / `language_for_path(path)` — language detection

## spell.rs — 379 lines
Spell checking via spellbook (Hunspell format).
### Types
- `SpellChecker` — dictionary + user dictionary
### Key Functions
- `SpellChecker::new(lang)` — load bundled dictionary
- `check_line(text, syntax_lang)` — find misspellings (tree-sitter-aware, LaTeX-aware)
- `suggest(word)` — spelling suggestions
- `add_word(word)` / `remove_word(word)` — user dictionary management

## extensions.rs — 353 lines
Bundled extension system.
### Types
- `BundledExtension` — name + manifest TOML + script files
- `ExtensionManifest` — parsed extension metadata (name, languages, LSP/DAP config, install commands)
### Key Functions
- `find_by_name(name)` / `find_for_file_ext(ext)` / `find_for_language_id(id)` — extension lookup
- `BUNDLED` — static array of 12 compiled-in extensions

## settings.rs — 2,206 lines
User settings with serde JSON persistence.
### Types
- `Settings` — all user-configurable settings (~40 fields with serde defaults)
### Key Functions
- `Settings::load()` — load from `~/.config/vimcode/settings.json` (returns default in tests)
- `Settings::save()` — write settings to disk
- `Settings::parse_set_option(arg)` — parse `:set` command arguments
- `get_value_str(name)` / `set_value_str(name, value)` — runtime get/set by name

## session.rs — 779 lines
Session state persistence (open files, layout, window positions).
### Types
- `SessionState` — serializable session data (groups, tabs, sidebar, terminal)
- `ExtensionState` — installed/dismissed extensions
### Key Functions
- `SessionState::load()` / `save()` — disk persistence
- `suppress_disk_saves()` — AtomicBool guard for tests

## project_search.rs — 631 lines
Recursive project-wide file search.
### Types
- `ProjectMatch` — search result (file, line, column, text)
- `ReplaceResult` — replace operation result
### Key Functions
- `search_in_project(root, query, options)` — search files respecting .gitignore
- `replace_in_project(root, query, replacement, options)` — search and replace

## registry.rs — 125 lines
Extension registry (GitHub-hosted JSON).
### Key Functions
- `fetch_registry(url)` — download extension registry
- `download_script(url, dest)` — download extension file via curl
