use super::sidebar::PANEL_EXTENSIONS;
use super::*;

/// Which half of a manifest's install (`[lsp]` or `[dap]`) a background
/// `tool_acquire` task is acquiring — carries what `finalize_tool_acquire`
/// needs to register the result (#1345).
pub(crate) enum ToolAcquireLeg {
    Lsp {
        lang_ids: Vec<String>,
        args: Vec<String>,
    },
    Dap,
}

/// Result of a completed background `tool_acquire::acquire_and_install`
/// call, sent back to the main thread over `Engine::tool_acquire_tasks`.
pub(crate) struct ToolAcquireOutcome {
    pub ext_name: String,
    pub install_key: String,
    pub leg: ToolAcquireLeg,
    pub tool_name: String,
    pub result: Result<std::path::PathBuf, String>,
    /// The specific notification this outcome must resolve — never marked
    /// done "by kind" (review finding on #1345): a manifest with both
    /// `[lsp.acquire]` and `[dap.acquire]` spawns two concurrent native
    /// acquisitions, both using `NotificationKind::LspInstall`, so
    /// `notify_done_by_kind` would mark *both* notifications done the moment
    /// either leg finishes — a still-downloading DAP adapter would flip to
    /// "done" in the UI the instant the LSP leg completes (or vice versa).
    pub notification_id: u64,
}

/// What `ext_install_from_registry` should do with a manifest's
/// `[lsp.acquire]`/`[dap.acquire]` table (#1346): kick off a native
/// background acquisition, or — for one of the five package-manager kinds
/// whose runtime isn't on PATH — fall through to the visible-terminal tier
/// instead of failing silently.
///
/// Pulled out as a pure function of an explicit `runtime_present` predicate
/// (mirrors `extensions::LspConfig::install_cmd_with`'s `rustup_available:
/// bool` seam) rather than probing PATH inline, so the fallback decision —
/// and, via `ext_install_from_registry_with_runtime_check`, the whole
/// engine-level branch it drives — is unit-testable without depending on
/// which of npm/python3/go/cargo/dotnet happen to be installed on the
/// machine running the test suite.
pub(crate) enum AcquireAction {
    /// Kick off a native acquisition: every archive-download kind, or a
    /// package-manager kind whose runtime is present.
    Native,
    /// Package-manager kind, runtime missing — fall through to the
    /// terminal tier. Carries the runtime's name and a ready-to-show
    /// install hint (from `extensions::prereq_install_cmd`, falling back to
    /// a generic phrasing for a runtime outside that table).
    NeedsRuntime { runtime: &'static str, hint: String },
}

pub(crate) fn resolve_acquire_action(
    acquire: &crate::core::tool_acquire::AcquireConfig,
    runtime_present: impl Fn(&str) -> bool,
) -> AcquireAction {
    match crate::core::tool_acquire::package_manager_runtime(acquire.kind) {
        Some(runtime) if !runtime_present(runtime) => {
            let hint = crate::core::extensions::prereq_install_cmd(runtime)
                .map(|cmd| format!("{runtime}: {cmd}"))
                .unwrap_or_else(|| format!("install {runtime} and try again"));
            AcquireAction::NeedsRuntime { runtime, hint }
        }
        _ => AcquireAction::Native,
    }
}

/// Per-extension aggregation state for the native acquisitions one
/// `:ExtInstall` kicked off (#1345 review follow-up).
///
/// A manifest with both `[lsp.acquire]` and `[dap.acquire]` spawns two
/// background threads that finish **whenever they finish** — the same
/// `poll_tool_acquire` tick if both are fast, different ticks otherwise
/// (a big download next to a small one, or just scheduler luck). Joining
/// only the outcomes that happen to land in the *same* tick therefore
/// fixes nothing on its own: the later tick's `self.message = …` still
/// erases the earlier tick's text, which is exactly the "DAP outcome
/// clobbering LSP success" bug (#1344) in slow motion. Carrying the
/// finished legs' text here, across ticks, until the last leg of that
/// extension reports, makes the final status line contain every leg's
/// outcome regardless of completion order or tick boundaries.
#[derive(Default)]
pub(crate) struct ToolAcquireGroup {
    /// Legs spawned for this extension that have not reported yet.
    pub pending: usize,
    /// Status text produced by each leg that has already reported, in
    /// completion order.
    pub messages: Vec<String>,
}

impl Engine {
    // =======================================================================
    // LSP integration
    // =======================================================================

    /// Ensure the LSP manager is initialized (lazy — created on first use).
    pub(crate) fn ensure_lsp_manager(&mut self) {
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
    pub(crate) fn lsp_did_open(&mut self, buffer_id: BufferId) {
        // Fire plugin "open" hook regardless of LSP enabled state
        if let Some(state) = self.buffer_manager.get(buffer_id) {
            let path = state.file_path.clone();
            let ft = state.lsp_language_id.clone();
            if let Some(path) = path {
                let path_str = path.to_string_lossy().into_owned();
                self.plugin_event("open", &path_str);
                self.plugin_event("BufNew", &path_str);
                self.plugin_event("BufEnter", &path_str);
                // #1623: FileType, once the language id is known (set by
                // `buffer_manager::open_file`'s path-based detection before
                // this runs). Arg is the filetype name — the whole point of
                // this event is `vimcode.on("FileType", function(ft) if ft
                // == "rust" then ... end end)`-style per-language setup,
                // matching vim's own `<amatch>` convention for it.
                if let Some(ft) = ft {
                    self.plugin_event("FileType", &ft);
                }
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
        if let Some(manifest) =
            crate::core::extensions::find_manifest_for_language_id(&manifests, &lang_id)
        {
            let name = &manifest.name;
            if !self.extension_state.is_installed(name)
                && !self.extension_state.is_dismissed(name)
                && !self.prompted_extensions.contains(name.as_str())
            {
                self.prompted_extensions.insert(name.to_string());
                // #1397/#1577: the offer is a non-modal, actionable toast
                // (never auto-expires — see `EngineToast::sticky`'s doc)
                // with real "Install"/"Don't ask again" buttons, not a
                // missable status-line hint or shortcuts printed as body
                // text. `:ExtInstall <name>` keeps working as the
                // command-line equivalent; keyboard-only users reach the
                // buttons through `:Notifications` /
                // `panel_keys.focus_notifications`
                // (`Engine::focus_toast_stack`) instead of a hijacked vim
                // key — the pre-#1577 `N` shortcut silently and
                // permanently dismissed the offer if the user pressed vim's
                // own `N` (search-previous) while it happened to be up.
                self.push_extension_recommendation_toast(name, &manifest.display_name);
            } else if let Some(err) = no_server {
                // #436: extension is installed but the LSP didn't start
                // (binary missing, install crashed, etc.).  Surface the
                // hint from `ensure_server_for_language` instead of
                // silently failing.
                self.message = err;
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
    pub fn ext_installed_manifests(&self) -> Vec<crate::core::extensions::ExtensionManifest> {
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| self.extension_state.is_installed(&m.name))
            .collect()
    }

    pub fn ext_available_manifests(&self) -> Vec<crate::core::extensions::ExtensionManifest> {
        let mut result: Vec<crate::core::extensions::ExtensionManifest> =
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
                    if let Some(manifest) =
                        crate::core::extensions::ExtensionManifest::parse(&toml_str)
                    {
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
    /// Result arrives via `ext_registry_rx`. Explicit/user-initiated refresh
    /// (Extensions panel open, its refresh key, `:ExtRefresh`) — surfaces a
    /// status message when it completes. Startup's own automatic refresh
    /// uses [`Self::ext_refresh_quiet`] instead; see that method's doc and
    /// `ext_registry_quiet`'s doc for why the two must not share a message
    /// policy (#1761).
    pub fn ext_refresh(&mut self) {
        self.ext_refresh_inner(false);
    }

    /// The automatic refresh [`Engine::startup_inner`] fires unconditionally
    /// on every launch, before anything the user did. Identical to
    /// [`Self::ext_refresh`] except the eventual completion — consumed by
    /// [`Self::poll_ext_registry`] — never touches `self.message` (#1761).
    ///
    /// That matters because this fetch's completion time is bounded only by
    /// `registry::fetch_registry`'s `curl --max-time 15` — anywhere from
    /// under a second (warm DNS/fast network) to ~15s (slow network, no
    /// cached registry to fall back to while it runs), entirely outside the
    /// user's control and uncorrelated with anything they did. A status
    /// message for it — even a successful, correct one — is still
    /// unsolicited output that can land several seconds after the first
    /// frame painted, breaking the `idle-no-repaint-bytes-when-idle`
    /// journey's contract that the terminal goes perfectly silent once
    /// idle. An explicit, user-requested refresh keeps its message: the
    /// user just took an action and is watching for its result, so staying
    /// silent *there* would be the bug.
    pub(crate) fn ext_refresh_quiet(&mut self) {
        self.ext_refresh_inner(true);
    }

    /// Shared body of [`Self::ext_refresh`] / [`Self::ext_refresh_quiet`].
    fn ext_refresh_inner(&mut self, quiet: bool) {
        if self.ext_registry_fetching {
            // Already in progress — dedupe against it rather than spawning a
            // second fetch. But if *this* caller is non-quiet (an explicit,
            // user-requested refresh) and the in-flight fetch was armed
            // quiet (startup's automatic one), upgrade the flag so the
            // fetch-in-progress still reports its result when it lands
            // (#1761): otherwise the user's `r` keypress produces no
            // feedback at all, silently inheriting startup's silence policy.
            if !quiet {
                self.ext_registry_quiet = false;
            }
            return;
        }
        let urls = self.settings.extension_registries.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut merged: Vec<crate::core::extensions::ExtensionManifest> = Vec::new();
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
        self.ext_registry_quiet = quiet;
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
            let quiet = self.ext_registry_quiet;
            self.ext_registry_quiet = false;
            match maybe_reg {
                Some(entries) => {
                    let count = entries.len();
                    registry::save_cache(&entries);
                    self.ext_registry = Some(entries);
                    // Re-filter stored diagnostics with updated
                    // ignore_error_sources. Note this can change painted
                    // output (gutter signs / Problems panel / status counts)
                    // even on a quiet fetch, and the redraw verdict below
                    // deliberately does not cover it: it only matters when a
                    // registry entry's `ignore_error_sources` changed *and*
                    // diagnostics were already on screen, and it self-heals on
                    // the next repaint (#1761 review).
                    self.refilter_diagnostics();
                    if !quiet {
                        self.message = format!("Extension registry updated ({count} extensions)");
                    }
                }
                None => {
                    if self.ext_registry.is_some() {
                        // Cache from a previous fetch is still available —
                        // silently keep it rather than alarming the user.
                    } else if !quiet {
                        self.message = "Registry fetch failed — try again later".to_string();
                    }
                }
            }
            // #1761: a quiet (startup) fetch reports "no redraw needed" so a
            // cold launch stays silent; see `Engine::ext_registry_quiet`'s doc
            // for why. The panel check is the one case where a quiet fetch
            // *is* visible: `sidebar.rs`'s `PANEL_EXTENSIONS` open handler
            // deliberately does not re-arm while a fetch is in flight, so a
            // panel opened during the fetch window relies on this completion
            // for its first painted list.
            !quiet || self.active_panel_is(PANEL_EXTENSIONS)
        } else {
            false
        }
    }

    /// Resolve the base URL for downloading extension files.
    /// Uses the manifest's `registry_base_url` if available, otherwise derives it
    /// from the first configured registry URL (the field is `#[serde(skip)]` so it's
    /// empty when loaded from cache).
    pub(crate) fn resolve_registry_base_url(
        &self,
        manifest: &crate::core::extensions::ExtensionManifest,
    ) -> String {
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
        self.ext_install_from_registry_with_runtime_check(name, binary_on_path)
    }

    /// The real body of `ext_install_from_registry`, parameterised over the
    /// "is this package-manager kind's runtime on PATH?" predicate (#1346).
    /// `ext_install_from_registry` calls this with the real `binary_on_path`
    /// — split out so a test can call this directly with a stub predicate
    /// and deterministically exercise the "runtime missing → terminal
    /// fallback" branch without depending on what's actually installed on
    /// the machine running the suite.
    ///
    /// `pub(crate)` (rather than private) specifically so the driver-tier
    /// black-box test in `tui_main::app_on_tui_tests` can drive this same
    /// deterministic seam through a real `TuiDriver` and assert on
    /// `driver.screen()` — the engine-internal-state test right below this
    /// one is necessary but not sufficient per CLAUDE.md's black-box
    /// coverage rule (#1346 review).
    pub(crate) fn ext_install_from_registry_with_runtime_check(
        &mut self,
        name: &str,
        runtime_present: impl Fn(&str) -> bool,
    ) {
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

        // #1807: refuse to install an extension whose `requires_vimcode`
        // constraint isn't met by the running vimcode — with a clear
        // message, rather than a silent no-op or an install that leaves
        // scripts erroring at load time on a `vimcode.*` API this version
        // doesn't have yet.
        if let Some(reason) = manifest.incompatibility_reason_for_running_vimcode() {
            let display = manifest.display_or_name();
            self.message = format!("Cannot install '{display}' — {reason}");
            return;
        }

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
        // #1345: true once any leg has kicked off a native acquisition
        // (download/verify/unpack, no terminal). Tracked separately from
        // `has_install` below because both can never involve the same leg —
        // a leg does the shared-resolver check, then EITHER native
        // acquisition OR the terminal install script, never both — but a
        // manifest with an LSP `acquire` and a DAP `install_*` (or vice
        // versa) can set both flags in the same call.
        let mut has_native_acquire = false;

        // ── LSP ──────────────────────────────────────────────────────────────
        // Resolution order (#1345): (1) already resolvable via the shared
        // tool lookup (`binary_on_path`, which now probes the vimcode-managed
        // tools dir first) → done; (2) manifest declares `[lsp.acquire]` →
        // native acquisition, no terminal, no shell; (3) else the legacy
        // `install_*` shell string in the visible terminal, unchanged.
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            let found_bin = all_lsp.iter().copied().find(|b| binary_on_path(b));
            if let Some(bin) = found_bin {
                status_parts.push(format!("LSP: {bin} ✓"));
            } else if let Some(acquire) = manifest.lsp.acquire.clone() {
                match resolve_acquire_action(&acquire, &runtime_present) {
                    AcquireAction::Native => {
                        let lsp_key = format!("ext:{ext_name}:lsp");
                        self.lsp_installing.insert(lsp_key.clone());
                        let notification_id = self.notify(
                            NotificationKind::LspInstall,
                            &format!("Acquiring {}…", manifest.lsp.binary),
                        );
                        self.spawn_tool_acquire(
                            ext_name.clone(),
                            lsp_key,
                            ToolAcquireLeg::Lsp {
                                lang_ids: manifest.language_ids.clone(),
                                args: manifest.lsp.args.clone(),
                            },
                            manifest.lsp.binary.clone(),
                            acquire,
                            notification_id,
                        );
                        has_native_acquire = true;
                        status_parts.push(format!("LSP: acquiring {}…", manifest.lsp.binary));
                    }
                    AcquireAction::NeedsRuntime { runtime, hint } => {
                        let lsp_key = format!("ext:{ext_name}:lsp");
                        self.lsp_installing.insert(lsp_key.clone());
                        install_commands.push(format!(
                            "echo 'Cannot acquire {} — {runtime} not found on PATH. {hint}'",
                            manifest.lsp.binary
                        ));
                        self.pending_install_context = Some(InstallContext {
                            ext_name: ext_name.clone(),
                            install_key: lsp_key,
                        });
                        status_parts.push(format!("LSP: needs {runtime} — {hint}"));
                    }
                }
            } else if !manifest.lsp.install_cmd_for_platform().is_empty() {
                // #1719: this is the legacy terminal-install leg — unlike
                // the `[lsp.acquire]` branch above, nothing here checked
                // `manifest.lsp.dependencies` before now. Detection only
                // ran at server-start time (`resolve_and_start_server`),
                // well after the install pane had already failed with a
                // bare "command not found: npm". Check first; show the
                // same actionable hint `missing_dependency_message`
                // already builds for the server-start case, and never
                // dispatch a doomed command.
                let missing: Vec<&str> = manifest
                    .lsp
                    .dependencies
                    .iter()
                    .filter(|dep| !runtime_present(dep))
                    .map(|s| s.as_str())
                    .collect();
                if missing.is_empty() {
                    let lsp_key = format!("ext:{ext_name}:lsp");
                    self.lsp_installing.insert(lsp_key.clone());
                    install_commands.push(manifest.lsp.install_cmd_for_platform().to_string());
                    self.pending_install_context = Some(InstallContext {
                        ext_name: ext_name.clone(),
                        install_key: lsp_key,
                    });
                    self.notify(
                        NotificationKind::LspInstall,
                        &format!("Installing {}…", manifest.lsp.binary),
                    );
                    status_parts.push(format!("LSP: installing {}…", manifest.lsp.binary));
                } else {
                    status_parts.push(crate::core::lsp_manager::missing_dependency_message(
                        &manifest, &missing,
                    ));
                }
            }
        }

        // `install_cmd_for_adapter` needs the manifest list to look up
        // manifest-declared installs; fetch once for the DAP block.
        let available_manifests = self.ext_available_manifests();

        // ── DAP ──────────────────────────────────────────────────────────────
        // Check PATH first (idempotent), then prefer native acquisition
        // (#1345) when the manifest declares `[dap.acquire]`, then fall back
        // to the unified resolver that knows about both manifest-declared
        // installs AND the built-in multi-step installers (codelldb, debugpy
        // venv, netcoredbg archive unpack). Previously this branch read
        // `manifest.dap.install` only, which is empty for adapters with
        // hardcoded installers — sending the user into a `:DapInstall <lang>`
        // loop that resolved back here.
        if !manifest.dap.adapter.is_empty() {
            let dap_binary = manifest.dap.binary.as_str();
            let already_on_path = !dap_binary.is_empty() && binary_on_path(dap_binary);
            if already_on_path {
                status_parts.push(format!("DAP: {dap_binary} ✓"));
            } else if let Some(acquire) = manifest.dap.acquire.clone() {
                match resolve_acquire_action(&acquire, &runtime_present) {
                    AcquireAction::Native => {
                        let dap_key = format!("dap:{}", manifest.dap.adapter);
                        self.lsp_installing.insert(dap_key.clone());
                        let notification_id = self.notify(
                            NotificationKind::LspInstall,
                            &format!("Acquiring {}…", manifest.dap.adapter),
                        );
                        self.spawn_tool_acquire(
                            ext_name.clone(),
                            dap_key,
                            ToolAcquireLeg::Dap,
                            manifest.dap.binary.clone(),
                            acquire,
                            notification_id,
                        );
                        has_native_acquire = true;
                        status_parts.push(format!("DAP: acquiring {}…", manifest.dap.adapter));
                    }
                    AcquireAction::NeedsRuntime { runtime, hint } => {
                        let dap_key = format!("dap:{}", manifest.dap.adapter);
                        self.lsp_installing.insert(dap_key.clone());
                        install_commands.push(format!(
                            "echo 'Cannot acquire {} — {runtime} not found on PATH. {hint}'",
                            manifest.dap.adapter
                        ));
                        if self.pending_install_context.is_none() {
                            self.pending_install_context = Some(InstallContext {
                                ext_name: ext_name.clone(),
                                install_key: dap_key,
                            });
                        }
                        status_parts.push(format!("DAP: needs {runtime} — {hint}"));
                    }
                }
            } else {
                // #1719: merge manifest-declared `dap.dependencies` with
                // the built-in adapters' hardcoded prerequisites
                // (`dap_manager::adapter_dependencies` — delve needs `go`,
                // debugpy needs `python3`, codelldb/netcoredbg need
                // `curl`+`unzip`/`tar`) *before* building the install
                // command, same treatment the LSP leg above just got.
                // The built-in list only applies when this manifest itself
                // has no `install_*` override — if it does, `install_cmd_
                // for_adapter` will use that command instead, which may
                // have entirely different prerequisites.
                let mut missing: Vec<String> = manifest
                    .dap
                    .dependencies
                    .iter()
                    .filter(|dep| !runtime_present(dep))
                    .cloned()
                    .collect();
                if manifest.dap.install_cmd_for_platform().is_empty() {
                    for dep in crate::core::dap_manager::adapter_dependencies(
                        &manifest.dap.adapter,
                        extensions::Platform::host(),
                    ) {
                        if !runtime_present(dep) && !missing.iter().any(|m| m == dep) {
                            missing.push(dep.to_string());
                        }
                    }
                }
                if !missing.is_empty() {
                    let missing_refs: Vec<&str> = missing.iter().map(String::as_str).collect();
                    status_parts.push(crate::core::lsp_manager::missing_dependency_message(
                        &manifest,
                        &missing_refs,
                    ));
                } else {
                    let adapter_install = crate::core::dap_manager::install_cmd_for_adapter(
                        manifest.dap.adapter.as_str(),
                        &available_manifests,
                    );
                    if let Some(cmd_str) = adapter_install {
                        let dap_key = format!("dap:{}", manifest.dap.adapter);
                        self.lsp_installing.insert(dap_key.clone());
                        install_commands.push(cmd_str);
                        // Only set install context if LSP didn't already set it.
                        if self.pending_install_context.is_none() {
                            self.pending_install_context = Some(InstallContext {
                                ext_name: ext_name.clone(),
                                install_key: dap_key,
                            });
                        }
                        status_parts.push(format!("DAP: installing {}…", manifest.dap.adapter));
                    } else if !dap_binary.is_empty() {
                        // Nothing knows how to install this adapter — fall back
                        // to telling the user.
                        status_parts.push(format!(
                            "DAP: {dap_binary} needs manual install (no automated installer)"
                        ));
                    }
                }
            }
        }

        // If there are install commands, combine them and store for the UI to run
        // in a visible terminal pane.
        let has_install = !install_commands.is_empty();
        if has_install {
            // Use `;` as separator — `&&` is not valid in PowerShell 5.x
            // (Windows default).  `;` works in both PowerShell and bash.
            let combined = install_commands.join(" ; ");
            let header = format!("echo '── Installing {ext_name} ──'");
            self.pending_terminal_command = Some(format!("{header} ; {combined}"));
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
        // Skip if an install/acquisition is pending — the binary isn't available yet;
        // LSP will be started when the install terminal (or `finalize_tool_acquire`,
        // #1345) completes.
        if !has_install && !has_native_acquire {
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

    /// Spawn a background thread that downloads, verifies and unpacks
    /// `tool_name` per `acquire` (#1345), off the UI thread. The result
    /// arrives via `tool_acquire_tasks`, drained by `poll_tool_acquire` —
    /// same shape as `Engine::ext_refresh`'s background registry fetch and
    /// `plugins.rs`'s `async_shell_tasks`.
    fn spawn_tool_acquire(
        &mut self,
        ext_name: String,
        install_key: String,
        leg: ToolAcquireLeg,
        tool_name: String,
        acquire: crate::core::tool_acquire::AcquireConfig,
        notification_id: u64,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let bg_tool_name = tool_name.clone();
        let bg_install_key = install_key.clone();
        // Register the leg before it can possibly report, so a finalize
        // that lands in the very next tick sees a non-zero `pending` and
        // keeps its sibling's text (see `ToolAcquireGroup`).
        self.tool_acquire_groups
            .entry(ext_name.clone())
            .or_default()
            .pending += 1;
        std::thread::spawn(move || {
            let result = crate::core::tool_acquire::acquire_and_install(&bg_tool_name, &acquire)
                .map_err(|e| e.to_string());
            let _ = tx.send(ToolAcquireOutcome {
                ext_name,
                install_key: bg_install_key,
                leg,
                tool_name: bg_tool_name,
                result,
                notification_id,
            });
        });
        self.tool_acquire_tasks.insert(install_key, rx);
    }

    /// Non-blocking check for completed background tool acquisitions.
    /// Call this from `poll_idle`. Returns `true` if a redraw is needed.
    pub fn poll_tool_acquire(&mut self) -> bool {
        let mut completed: Vec<(String, ToolAcquireOutcome)> = Vec::new();
        for (key, rx) in &self.tool_acquire_tasks {
            if let Ok(outcome) = rx.try_recv() {
                completed.push((key.clone(), outcome));
            }
        }
        if completed.is_empty() {
            return false;
        }
        for (key, _) in &completed {
            self.tool_acquire_tasks.remove(key);
        }

        // Group by extension (review finding on #1345): a manifest with
        // both `[lsp.acquire]` and `[dap.acquire]` spawns two concurrent
        // background acquisitions, and if both land in the same poll tick,
        // finalizing them one at a time straight into `self.message` would
        // let the second overwrite the first's success/failure text —
        // exactly the "DAP outcome clobbering LSP success" class fixed for
        // the terminal-install path in `finalize_install_from_terminal`
        // (#1344). Outcomes for the same extension are collected and joined
        // into one status line instead — and, because two legs need not
        // land in the same tick at all, that join is carried across ticks
        // in `Engine::tool_acquire_groups` (see `ToolAcquireGroup`);
        // outcomes for different extensions
        // still each get their own call (and so the last one to finalize
        // wins `self.message` — a pre-existing, unrelated property of a
        // single-line status bar shared across all engine operations).
        let mut order: Vec<String> = Vec::new();
        let mut by_ext: std::collections::HashMap<String, Vec<ToolAcquireOutcome>> =
            std::collections::HashMap::new();
        for (_, outcome) in completed {
            if !by_ext.contains_key(&outcome.ext_name) {
                order.push(outcome.ext_name.clone());
            }
            by_ext
                .entry(outcome.ext_name.clone())
                .or_default()
                .push(outcome);
        }
        for ext_name in order {
            if let Some(outcomes) = by_ext.remove(&ext_name) {
                self.finalize_tool_acquire_group(outcomes);
            }
        }
        true
    }

    /// Apply the results of one or more completed background acquisitions
    /// belonging to the same extension: on success, register + start the
    /// LSP server (mirrors `terminal_ops::finalize_install_from_terminal`'s
    /// LSP branch) or, for a DAP leg, just report success — there is no
    /// long-lived adapter process to start here; `DapManager::start_adapter`
    /// resolves the adapter binary at debug-start time via
    /// `dap_manager::resolve_binary`, which (since this issue) delegates to
    /// the same `lsp_manager::resolve_command` that probes the
    /// vimcode-managed tools dir this acquisition just populated, so the
    /// success message painted here and the F5 launch agree. On failure,
    /// delete nothing further (`tool_acquire::acquire_and_install` already
    /// cleaned up its own partial state) and surface the error. Each
    /// outcome resolves its own notification by ID (never "by kind" — see
    /// `ToolAcquireOutcome::notification_id`'s doc comment), and the
    /// per-leg messages are accumulated in that extension's
    /// [`ToolAcquireGroup`] and re-joined into one status line on every
    /// finalize — mirroring `finalize_install_from_terminal`'s
    /// collect-then-join pattern, but *across ticks* so a second leg that
    /// finishes a tick later can never silently erase the first's text
    /// either (see `ToolAcquireGroup`'s doc comment).
    fn finalize_tool_acquire_group(&mut self, outcomes: Vec<ToolAcquireOutcome>) {
        let mut affected: Vec<String> = Vec::new();
        for outcome in outcomes {
            let mut messages: Vec<String> = Vec::new();
            self.lsp_installing.remove(&outcome.install_key);
            self.notify_done(outcome.notification_id, None);

            let ext_name = outcome.ext_name.clone();
            match outcome.result {
                Ok(bin_path) => {
                    crate::core::lsp_manager::install_log(&format!(
                        "[ext-install] '{ext_name}' acquired {} -> {}",
                        outcome.tool_name,
                        bin_path.display()
                    ));
                    match outcome.leg {
                        ToolAcquireLeg::Lsp { lang_ids, args } => {
                            self.ensure_lsp_manager();
                            for lsp_lang in &lang_ids {
                                let config = lsp::LspServerConfig {
                                    command: outcome.tool_name.clone(),
                                    args: args.clone(),
                                    languages: vec![lsp_lang.clone()],
                                    ..Default::default()
                                };
                                if let Some(mgr) = &mut self.lsp_manager {
                                    mgr.add_registry_entry(config);
                                    mgr.ensure_server_for_language(lsp_lang);
                                }
                                self.lsp_reopen_buffers_for_language(lsp_lang);
                            }
                            messages.push(format!(
                                "LSP server for '{ext_name}' installed and started ({})",
                                outcome.tool_name
                            ));
                        }
                        ToolAcquireLeg::Dap => {
                            messages.push(format!(
                                "DAP adapter for '{ext_name}' installed — press F5 to debug"
                            ));
                        }
                    }
                }
                Err(e) => {
                    crate::core::lsp_manager::install_log(&format!(
                        "[ext-install] '{ext_name}' acquisition of {} failed: {e}",
                        outcome.tool_name
                    ));
                    messages.push(format!(
                        "Install for '{ext_name}' failed to acquire {}: {e}",
                        outcome.tool_name
                    ));
                }
            }

            let group = self
                .tool_acquire_groups
                .entry(ext_name.clone())
                .or_default();
            group.pending = group.pending.saturating_sub(1);
            group.messages.append(&mut messages);
            if !affected.contains(&ext_name) {
                affected.push(ext_name);
            }
        }

        for ext_name in affected {
            let Some(group) = self.tool_acquire_groups.get(&ext_name) else {
                continue;
            };
            if !group.messages.is_empty() {
                self.message = group.messages.join(" | ");
            }
            // Last leg of this extension reported — drop the accumulator so
            // a later re-install of the same extension starts from a clean
            // slate rather than re-painting the previous run's outcomes.
            if group.pending == 0 {
                self.tool_acquire_groups.remove(&ext_name);
            }
        }
    }

    /// Open the README for the currently selected extension in the sidebar.
    /// Used by Enter and double-click.
    pub fn ext_open_selected_readme(&mut self) {
        let manifests = self.ext_available_manifests();
        let (in_installed, idx) = self.ext_selected_from_sidebar_system();
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
    pub(crate) fn ext_show_remove_dialog(&mut self, name: &str) {
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

        // Ensure the available section is visible after removal so the
        // user can see where the extension moved.
        self.ext_sidebar_system.borrow_mut().set_collapsed(1, false);
    }

    /// Remove LSP/DAP tool binaries installed by an extension.
    /// Only removes binaries found under well-known managed directories
    /// (~/.local/bin, ~/.local/share/<name>, Mason bin dir).
    pub(crate) fn ext_remove_tools(&mut self, name: &str) {
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
            // #1345 (review): every path below is built by joining this
            // manifest-supplied name onto a directory vimcode then *deletes
            // from* (`remove_dir_all` for the managed tool dir and the
            // `~/.local/share/<name>` data dir, `remove_file` in the safe
            // dirs). `binary` is free-form text out of a community-submitted
            // registry manifest and `PathBuf::join` resolves nothing, so
            // `binary = "../.."` would otherwise make `:ExtUninstall` with
            // "remove tools" recursively delete an *ancestor* of
            // `~/.local/share/vimcode/tools` — with enough segments, the
            // user's home directory. Gate on the same predicate
            // `tool_acquire::install_resolved_asset` already applies to this
            // identical value on the way in.
            if !crate::core::tool_acquire::is_safe_single_segment_name(bin_name) {
                crate::core::lsp_manager::install_log(&format!(
                    "[ext-remove] Refusing to remove tools for unsafe binary \
                     name {bin_name:?} declared by '{name}' — not a plain \
                     file name"
                ));
                continue;
            }
            // #1345: delete the vimcode-managed acquisition dir outright —
            // unlike the shared `safe_dirs` above (system directories other
            // tools might also use), `tools/<bin_name>/` is exclusively
            // populated by `tool_acquire::acquire_and_install`, so removing
            // the whole directory (every version, not just `current`) can
            // never delete anything vimcode doesn't own.
            let managed_dir = paths::managed_tool_dir(bin_name);
            if managed_dir.is_dir() && std::fs::remove_dir_all(&managed_dir).is_ok() {
                removed.push(format!("{}", managed_dir.display()));
            }
            // Remove binary from safe dirs.
            for dir in &safe_dirs {
                let path = dir.join(bin_name);
                if !path.exists() {
                    continue;
                }
                // #436: ~/.cargo/bin/<binary> may be a symlink to `rustup`
                // (rustup creates these proxies for every component binary,
                // including rust-analyzer).  Removing the proxy strands the
                // rustup-managed binary — `rustup component add` won't
                // recreate it, so the user has to do `ln -s rustup …` by
                // hand.  Skip rustup proxies and leave them in place.
                if is_rustup_proxy(&path) {
                    crate::core::lsp_manager::install_log(&format!(
                        "[ext-remove] Skipping rustup proxy at {}",
                        path.display()
                    ));
                    continue;
                }
                if std::fs::remove_file(&path).is_ok() {
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
            crate::core::lsp_manager::install_log(&format!(
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

        // #1807: an update is the most realistic route to an incompatible
        // installed extension — the registry bumping `requires_vimcode`
        // in a newer manifest version is exactly the shape of change this
        // field exists for. Refuse the same way the initial install does,
        // leaving the already-installed (compatible) version untouched.
        if let Some(reason) = manifest.incompatibility_reason_for_running_vimcode() {
            let display = manifest.display_or_name();
            self.message = format!("Cannot update '{display}' — {reason}");
            return;
        }

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
            let combined = install_commands.join(" ; ");
            let header = format!("echo '── Updating {ext_name} ──'");
            self.pending_terminal_command = Some(format!("{header} ; {combined}"));
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
        // #1807: split out any candidate whose newer manifest is
        // incompatible with the running vimcode — same reasoning as
        // `ext_update_one`, applied per-extension here since `:ExtUpdate`
        // batches every installed extension with an available update.
        let mut skipped_incompatible: Vec<String> = Vec::new();
        let mut actually_updated = Vec::new();
        for name in &updated {
            if let Some(manifest) = manifests.iter().find(|m| &m.name == name) {
                if let Some(reason) = manifest.incompatibility_reason_for_running_vimcode() {
                    skipped_incompatible.push(format!("{} ({reason})", manifest.display_or_name()));
                    continue;
                }
            }
            actually_updated.push(name.clone());
        }
        for name in &actually_updated {
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
        let count = actually_updated.len();
        self.message = if skipped_incompatible.is_empty() {
            format!(
                "{count} extension(s) updated: {}",
                actually_updated.join(", ")
            )
        } else {
            format!(
                "{count} extension(s) updated: {}; skipped incompatible: {}",
                actually_updated.join(", "),
                skipped_incompatible.join(", ")
            )
        };
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

    /// Get the LSP status for a specific buffer's language.
    /// Returns `LspStatus::None` if no LSP is configured or the manager isn't started.
    pub fn lsp_status_for_buffer(
        &self,
        buffer_id: crate::core::buffer::BufferId,
    ) -> crate::core::lsp_manager::LspStatus {
        use crate::core::lsp_manager::LspStatus;
        let buf = match self.buffer_manager.get(buffer_id) {
            Some(s) => s,
            None => return LspStatus::None,
        };
        let lang = match buf.lsp_language_id.as_deref() {
            Some(l) => l,
            None => return LspStatus::None,
        };
        if self.lsp_installing.contains(lang) {
            return LspStatus::Installing;
        }
        let mgr = match &self.lsp_manager {
            Some(m) => m,
            None => return LspStatus::None,
        };
        let status = mgr.lsp_status_for_language(lang);
        // #450: keep the indicator dimmed (`name…`) while the server has
        // any open `$/progress` work item. This is the LSP-protocol signal
        // for "still indexing the workspace" — rust-analyzer / gopls /
        // pyright all emit it. Previously we tried two semantic-tokens
        // heuristics:
        //   - `semantic_tokens.is_empty()` (Session 243) — pinned forever
        //     for files with no tokens (#230).
        //   - `!semantic_tokens_received` (#230 fix) — went bright on the
        //     first empty response, well before indexing actually finished.
        // The progress-notification gate is both. Servers that don't emit
        // progress notifications (marksman, simple servers) brighten on
        // handshake — same as before.
        if let LspStatus::Running(name) = &status {
            if let Some(server_id) = mgr.server_id_for_language(lang) {
                if mgr.is_indexing(server_id) {
                    return LspStatus::Initializing(name.clone());
                }
            }
        }
        status
    }

    /// Return the active `$/progress` snapshot for the buffer's server
    /// (#221). Used by the status bar to format
    /// `name • Indexing: 319/320` segments. Returns None when no
    /// progress is open or the manager/language isn't set up.
    pub fn lsp_progress_for_buffer(
        &self,
        buffer_id: crate::core::buffer::BufferId,
    ) -> Option<crate::core::lsp_manager::LspProgress> {
        let buf = self.buffer_manager.get(buffer_id)?;
        let lang = buf.lsp_language_id.as_deref()?;
        let mgr = self.lsp_manager.as_ref()?;
        let server_id = mgr.server_id_for_language(lang)?;
        mgr.current_progress(server_id).cloned()
    }
}

/// Returns true if `path` is a symlink pointing to `rustup` (the rustup
/// proxy used to dispatch component binaries like `rust-analyzer`,
/// `rustc`, `rustfmt`).  These proxies are created by rustup and cannot
/// be restored by `rustup component add` once removed.
#[cfg(not(target_os = "windows"))]
fn is_rustup_proxy(path: &Path) -> bool {
    match std::fs::read_link(path) {
        Ok(target) => {
            // Target is just `rustup` (relative symlink, the common case)
            // or an absolute path ending in `/rustup`.
            target == Path::new("rustup")
                || target.file_name().map(|n| n == "rustup").unwrap_or(false)
        }
        Err(_) => false,
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod rustup_proxy_tests {
    use super::is_rustup_proxy;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    fn scratch_dir(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("vimcode-rustup-test-{tag}-{pid}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn symlink_to_relative_rustup_is_proxy() {
        // #436: rustup creates `~/.cargo/bin/rust-analyzer -> rustup`
        // (relative symlink).  ext_remove_tools must skip these.
        let dir = scratch_dir("relative");
        let link = dir.join("rust-analyzer");
        symlink("rustup", &link).unwrap();
        assert!(is_rustup_proxy(&link));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn symlink_to_absolute_rustup_is_proxy() {
        let dir = scratch_dir("absolute");
        let link = dir.join("rust-analyzer");
        symlink("/usr/local/bin/rustup", &link).unwrap();
        assert!(is_rustup_proxy(&link));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_file_is_not_proxy() {
        // A real binary (not a symlink) must be removed normally.
        let dir = scratch_dir("real");
        let bin = dir.join("codelldb");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
        assert!(!is_rustup_proxy(&bin));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn symlink_to_other_target_is_not_proxy() {
        // A symlink to a non-rustup target should still be removable.
        let dir = scratch_dir("other");
        let target = dir.join("some-binary");
        std::fs::write(&target, b"").unwrap();
        let link = dir.join("rust-analyzer");
        symlink(&target, &link).unwrap();
        assert!(!is_rustup_proxy(&link));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// On Windows rustup proxies are hard-linked copies of rustup.exe;
/// detect by matching file size against the sibling `rustup.exe`.
/// Falls back to false on any I/O error — better to over-delete than
/// to leak unmanaged binaries.
#[cfg(target_os = "windows")]
fn is_rustup_proxy(path: &Path) -> bool {
    let parent = match path.parent() {
        Some(p) => p,
        None => return false,
    };
    let rustup_exe = parent.join("rustup.exe");
    if !rustup_exe.exists() {
        return false;
    }
    let proxy_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let rustup_len = std::fs::metadata(&rustup_exe).map(|m| m.len()).unwrap_or(0);
    proxy_len != 0 && proxy_len == rustup_len
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a finished-leg outcome without running a real acquisition.
    fn outcome(
        ext: &str,
        install_key: &str,
        leg: ToolAcquireLeg,
        tool: &str,
        result: Result<std::path::PathBuf, String>,
    ) -> ToolAcquireOutcome {
        ToolAcquireOutcome {
            ext_name: ext.to_string(),
            install_key: install_key.to_string(),
            leg,
            tool_name: tool.to_string(),
            result,
            notification_id: 0,
        }
    }

    // ── #1346: package-manager runtime dependency → terminal fallback ────

    /// Pure decision-table test for `resolve_acquire_action` — the seam
    /// `ext_install_from_registry_with_runtime_check` drives for every
    /// `[lsp.acquire]`/`[dap.acquire]` table. No PATH probing, no process
    /// spawn: `runtime_present` is passed in explicitly, so this exercises
    /// every kind × presence combination deterministically regardless of
    /// what's actually installed on the machine running the suite.
    #[test]
    fn resolve_acquire_action_decision_table() {
        use crate::core::tool_acquire::{AcquireConfig, AcquireKind};

        // Archive-download kinds: always `Native`, whatever the predicate
        // says — `package_manager_runtime` returns `None` for these, so the
        // predicate is never even consulted.
        for kind in [
            AcquireKind::HashicorpRelease,
            AcquireKind::GithubRelease,
            AcquireKind::UrlTemplate,
        ] {
            let cfg = AcquireConfig {
                kind,
                ..Default::default()
            };
            assert!(matches!(
                resolve_acquire_action(&cfg, |_| false),
                AcquireAction::Native
            ));
            assert!(matches!(
                resolve_acquire_action(&cfg, |_| true),
                AcquireAction::Native
            ));
        }

        // Package-manager kinds: `Native` when the runtime is present,
        // `NeedsRuntime` (naming that exact runtime) when it isn't.
        for (kind, runtime) in [
            (AcquireKind::Npm, "npm"),
            (AcquireKind::Pip, "python3"),
            (AcquireKind::Go, "go"),
            (AcquireKind::Cargo, "cargo"),
            (AcquireKind::DotnetTool, "dotnet"),
        ] {
            let cfg = AcquireConfig {
                kind,
                package: "irrelevant".to_string(),
                ..Default::default()
            };
            assert!(
                matches!(
                    resolve_acquire_action(&cfg, |_| true),
                    AcquireAction::Native
                ),
                "{kind:?} with its runtime present should be Native"
            );
            match resolve_acquire_action(&cfg, |_| false) {
                AcquireAction::NeedsRuntime {
                    runtime: got_runtime,
                    hint,
                } => {
                    assert_eq!(got_runtime, runtime);
                    assert!(
                        !hint.is_empty(),
                        "{kind:?}'s missing-runtime hint must not be empty"
                    );
                }
                AcquireAction::Native => {
                    panic!("{kind:?} with its runtime absent should be NeedsRuntime")
                }
            }
        }
    }

    /// #1346 acceptance criterion: a manifest whose `[lsp.acquire]` names a
    /// package-manager kind whose runtime isn't on PATH must fall through
    /// to the visible-terminal tier with a dependency hint — never spawn a
    /// native acquisition (which would just fail silently, or worse, spawn
    /// a process that doesn't exist) and never leave the user with no
    /// feedback at all.
    ///
    /// `runtime_present` is stubbed to always return `false` — deterministic
    /// regardless of whether the machine running this suite happens to have
    /// npm on PATH — so this drives the real engine method
    /// (`ext_install_from_registry_with_runtime_check`, the seam
    /// `ext_install_from_registry` itself calls with the real
    /// `binary_on_path`), not just the pure decision function above.
    ///
    /// Verified RED against a version of `ext_install_from_registry` that
    /// unconditionally spawns `spawn_tool_acquire` for any `[lsp.acquire]`
    /// table (the pre-#1346 behaviour, still correct for the three
    /// archive-download kinds): `tool_acquire_groups` gains an entry and
    /// `pending_terminal_command` stays `None`.
    #[test]
    fn missing_package_manager_runtime_falls_through_to_terminal_tier() {
        use crate::core::extensions::{ExtensionManifest, LspConfig};
        use crate::core::tool_acquire::{AcquireConfig, AcquireKind};

        let mut e = Engine::new();
        let ext_name = "vc-unit-pkgmgr-fallback-1346";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "Package-manager fallback test (#1346)".to_string(),
            language_ids: vec!["vc-unit-pkgmgr-1346-lang".to_string()],
            lsp: LspConfig {
                binary: "vc-unit-pkgmgr-1346-lsp".to_string(),
                acquire: Some(AcquireConfig {
                    kind: AcquireKind::Npm,
                    package: "vc-unit-pkgmgr-1346-pkg".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        }]);

        e.ext_install_from_registry_with_runtime_check(ext_name, |_| false);

        assert!(
            e.tool_acquire_groups.is_empty(),
            "a missing runtime must never spawn a native acquisition"
        );
        let cmd = e.pending_terminal_command.clone().unwrap_or_default();
        assert!(
            cmd.contains("npm"),
            "terminal fallback must name the missing runtime; got: {cmd}"
        );
        assert!(
            cmd.to_lowercase().contains("node"),
            "terminal fallback must carry npm's install hint (not just fail \
             silently); got: {cmd}"
        );
        assert!(
            e.message.contains("needs npm"),
            "status line must explain why native acquisition was skipped; got: {}",
            e.message
        );
    }

    /// #1345 review follow-up, deterministic half: two legs of the same
    /// `:ExtInstall` that report in **different** `poll_tool_acquire`
    /// ticks must both survive in the status line. The driver-tier test
    /// `tui_main::shell_app::tests::extension_install_lsp_acquire_success_
    /// survives_dap_acquire_failure_via_shell_app` covers the same rule on
    /// painted output, but it cannot *force* the two-tick ordering — the
    /// background threads decide that — so it reproduced the bug only
    /// intermittently (2 of 4 full `--lib` runs). Calling finalize twice
    /// here pins it.
    ///
    /// Verified RED against the same-tick-only join (`let mut messages`
    /// local to `finalize_tool_acquire_group`, assigned straight into
    /// `self.message`): the second call overwrites the first's text and
    /// the DAP assertion below fails.
    ///
    /// The DAP leg succeeds and the LSP leg fails (rather than the other
    /// way round) purely so neither call reaches `ensure_lsp_manager` /
    /// `ensure_server_for_language` — no process spawn, no PATH probing,
    /// nothing environment-dependent in a unit test.
    #[test]
    fn tool_acquire_outcomes_in_separate_ticks_keep_both_messages() {
        let mut e = Engine::new();
        let ext = "vc-unit-acq-ext-1345";

        // Two legs in flight, as `spawn_tool_acquire` would have left it.
        e.tool_acquire_groups
            .entry(ext.to_string())
            .or_default()
            .pending = 2;

        // Tick 1: the DAP leg lands on its own.
        e.finalize_tool_acquire_group(vec![outcome(
            ext,
            &format!("dap:{ext}"),
            ToolAcquireLeg::Dap,
            "vc-unit-acq-dap-1345",
            Ok(std::path::PathBuf::from(
                "/nonexistent/vc-unit-acq-dap-1345",
            )),
        )]);
        assert!(
            e.message.contains("DAP adapter") && e.message.contains(ext),
            "first leg must paint its own outcome; got: {}",
            e.message
        );
        assert_eq!(
            e.tool_acquire_groups.get(ext).map(|g| g.pending),
            Some(1),
            "the still-running LSP leg must keep the accumulator alive"
        );

        // Tick 2 (a separate `poll_tool_acquire` call): the LSP leg fails.
        e.finalize_tool_acquire_group(vec![outcome(
            ext,
            &format!("ext:{ext}:lsp"),
            ToolAcquireLeg::Lsp {
                lang_ids: vec![],
                args: vec![],
            },
            "vc-unit-acq-lsp-1345",
            Err("boom".to_string()),
        )]);
        assert!(
            e.message.contains("failed to acquire"),
            "second leg's outcome must reach the status line; got: {}",
            e.message
        );
        assert!(
            e.message.contains("DAP adapter"),
            "a leg finishing a tick later must not erase the earlier leg's \
             text; got: {}",
            e.message
        );

        // Last leg reported — accumulator dropped so a re-install starts clean.
        assert!(
            !e.tool_acquire_groups.contains_key(ext),
            "accumulator must be cleared once every leg has reported"
        );
    }

    /// #1345 blocking review finding: `ext_remove_tools` joins
    /// `manifest.lsp.binary` / `manifest.dap.binary` — free-form text from a
    /// community-submitted registry manifest — straight onto directories it
    /// then `remove_dir_all`s. `PathBuf::join` resolves nothing, so a
    /// manifest declaring `binary = "../.."` used to turn ":ExtUninstall,
    /// remove tools" into a recursive delete of an *ancestor* of
    /// `~/.local/share/vimcode/tools`.
    ///
    /// The sentinel here sits two levels above the managed tools dir — i.e.
    /// exactly where `managed_tool_dir("../../<sentinel>")` lands once the
    /// OS resolves the join — and must survive the removal. The traversal
    /// deliberately cannot reach anything real through the `$HOME`-based
    /// legs of that same loop either: `~/.local/bin/../../<sentinel>` and
    /// `~/.local/share/../../<sentinel>` resolve to a pid-unique name
    /// directly under `$HOME`'s parent that no machine has, so an unguarded
    /// (RED) run of this test deletes the throwaway sentinel and nothing
    /// else.
    ///
    /// **Verified RED without the guard:** removing the
    /// `is_safe_single_segment_name` check from `ext_remove_tools` makes the
    /// sentinel directory (and the file inside it) gone by the time the
    /// assertions run.
    ///
    /// Unit- rather than driver-tier on purpose: the guard changes no
    /// painted output at all — `ext_remove` reports the same "Extension 'x'
    /// and its tools removed" message either way — so what has to be
    /// asserted is the filesystem effect, which no screen can show.
    #[test]
    fn ext_remove_tools_refuses_path_traversal_binary_names() {
        use crate::core::extensions::{DapConfig, ExtensionManifest, LspConfig};

        let _lock = crate::core::paths::VIMCODE_TEST_DATA_HOME_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let unique = format!(
            "vimcode-test-1345-traversal-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        )
        .replace(['(', ')', ' '], "");
        let base = std::env::temp_dir().join(&unique);
        let data_home = base.join("data");
        let sentinel = base.join("sentinel");
        std::fs::create_dir_all(&sentinel).unwrap();
        std::fs::write(sentinel.join("keep-me.txt"), b"keep").unwrap();
        // `<data_home>/tools/<binary>` with `binary = "../../sentinel"`
        // resolves to `<base>/sentinel`.
        std::fs::create_dir_all(data_home.join("tools")).unwrap();

        let old_data_home = std::env::var_os("VIMCODE_TEST_DATA_HOME");
        std::env::set_var("VIMCODE_TEST_DATA_HOME", &data_home);

        let mut e = Engine::new();
        let ext_name = "vc-unit-traversal-ext-1345";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "Hostile manifest (1345 traversal test)".to_string(),
            language_ids: vec!["vc-unit-traversal-lang-1345".to_string()],
            lsp: LspConfig {
                // `..`-escape: `remove_dir_all` of the whole sentinel dir.
                binary: "../../sentinel".to_string(),
                ..Default::default()
            },
            dap: DapConfig {
                adapter: "vc-unit-traversal-adapter-1345".to_string(),
                // Nested-name escape: reaches a single file inside it.
                binary: "../../sentinel/keep-me.txt".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);

        e.ext_remove_tools(ext_name);

        match old_data_home {
            Some(v) => std::env::set_var("VIMCODE_TEST_DATA_HOME", v),
            None => std::env::remove_var("VIMCODE_TEST_DATA_HOME"),
        }

        assert!(
            sentinel.is_dir(),
            "a manifest binary name containing `..` must never make tool \
             removal delete a directory outside the managed tools tree; \
             {} is gone",
            sentinel.display()
        );
        assert!(
            sentinel.join("keep-me.txt").is_file(),
            "the sentinel directory survived but its contents did not — \
             the traversal still reached inside it"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// The predicate the guard above delegates to, spelled out: a plain
    /// file name is the only accepted shape. `is_safe_relative_path` alone
    /// (which `tool_acquire`'s archive-entry checks use) accepts `a/b`, and
    /// a nested name is just as much an escape as `..` when it is joined
    /// onto a directory that is about to be deleted.
    #[test]
    fn is_safe_single_segment_name_accepts_only_plain_file_names() {
        use crate::core::tool_acquire::is_safe_single_segment_name;
        for good in ["rust-analyzer", "clangd", "terraform-ls", "gopls.exe"] {
            assert!(
                is_safe_single_segment_name(good),
                "{good:?} must be accepted"
            );
        }
        for bad in [
            "",
            "..",
            ".",
            "../..",
            "../../../../",
            "a/b",
            "a\\b",
            "/etc",
            "./x",
            "sub/../..",
        ] {
            assert!(
                !is_safe_single_segment_name(bad),
                "{bad:?} must be rejected"
            );
        }
    }

    /// #1386 core acceptance: opening several files of a language whose
    /// server resolves to a broken proxy (present on disk, fails its
    /// `--version` probe — the rustup-uninstalled-component shape) must not
    /// re-run that probe for every open. `Engine::open_file_with_mode`
    /// calls `lsp_did_open`, which calls `ensure_server_for_language`
    /// *twice* per open (did-open + `lsp_request_semantic_tokens`), so
    /// without the negative cache this reads N×2 (or N×4, counting
    /// `resolve_command`'s own internal duplicate probe) instead of 1.
    ///
    /// Also proves the file still opens successfully (buffer content
    /// present, no error `Result`) despite the unresolvable server — the
    /// bug this issue reports was a UI-thread stall on open, not a hard
    /// failure, so a passing `open_file_with_mode` call on its own wouldn't
    /// be new coverage; the probe count is the actual regression guard.
    ///
    /// **Verified RED against unfixed `develop`:** reverting the
    /// `failed_language_resolutions` cache (`lsp_manager.rs`) makes
    /// `probe_count` below grow with each open instead of staying at 1.
    #[test]
    #[cfg(unix)]
    fn opening_multiple_files_with_unresolvable_lsp_server_probes_only_once() {
        let binary_name = "vimcode-test-1386-engine-proxy";
        let unique = format!(
            "vimcode_test_1386_engine_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(&unique);
        let home = base.join("home");
        let cargo_bin = home.join(".cargo").join("bin");
        let files_dir = base.join("files");
        std::fs::create_dir_all(&cargo_bin).unwrap();
        std::fs::create_dir_all(&files_dir).unwrap();
        let counter_file = base.join("probes.log");

        let binary_path = cargo_bin.join(binary_name);
        std::fs::write(
            &binary_path,
            format!(
                "#!/bin/sh\necho probe >> {}\nexit 1\n",
                counter_file.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Thread-local, not `set_var("HOME", …)` — see
        // `core::paths::TEST_HOME_OVERRIDE` for why the process-global
        // version corrupts concurrently-running tests (#957 smoke).
        let _home_guard = crate::core::paths::set_test_home(&home);

        let ext_name = "vc-test-1386-engine-ext";
        // Reuse the built-in `.java` → "java" mapping (`lsp::language_id_
        // from_path`) so this test needs no change to that table — only the
        // manifest's `lsp.binary` needs to be unresolvable.
        let mut e = Engine::new();
        e.ext_registry = Some(vec![crate::core::extensions::ExtensionManifest {
            name: ext_name.to_string(),
            display_name: ext_name.to_string(),
            language_ids: vec!["java".to_string()],
            file_extensions: vec![".java".to_string()],
            lsp: crate::core::extensions::LspConfig {
                binary: binary_name.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);
        e.extension_state.mark_installed(ext_name);

        for i in 0..3 {
            let file = files_dir.join(format!("File{i}.java"));
            std::fs::write(&file, format!("class File{i} {{}}\n")).unwrap();
            e.open_file_with_mode(&file, OpenMode::Permanent)
                .unwrap_or_else(|e| panic!("open must succeed despite unresolvable LSP: {e}"));
        }

        // The buffer actually opened — the bug was a UI stall, not a
        // failure to open.
        assert!(
            e.active_buffer_state().buffer.to_string().contains("File2"),
            "the third file's content must be present in the active buffer"
        );

        let probes = std::fs::read_to_string(&counter_file).unwrap_or_default();
        let probe_count = probes.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(
            probe_count, 1,
            "opening 3 files of a language whose server can't be resolved \
             must probe the broken proxy at most once total, not once per \
             open; got {probe_count} probes; log:\n{probes}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    // ── #1719: detect missing prerequisites before dispatching an install ──

    /// #1719 acceptance criterion: an extension whose `lsp.dependencies`
    /// names a binary that isn't on PATH must never reach the terminal
    /// pane at all — the pre-#1719 behaviour let the legacy `install_*`
    /// string through unconditionally, so the failure only ever showed up
    /// as `command not found: npm` *inside* the pane the install already
    /// started. `missing_dependency_message`'s actionable hint must appear
    /// in the status line instead, and no terminal command may be queued.
    ///
    /// Verified RED against the pre-#1719 code (the `else if !manifest.
    /// lsp.install_cmd_for_platform().is_empty()` branch with no `missing`
    /// check in front of it): `pending_terminal_command` becomes `Some`
    /// and `e.message` never mentions `npm`.
    #[test]
    fn lsp_legacy_install_blocked_when_declared_dependency_missing() {
        use crate::core::extensions::{ExtensionManifest, LspConfig};

        let mut e = Engine::new();
        let ext_name = "vc-unit-1719-lsp-missing-dep";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1719 LSP missing-dep test".to_string(),
            language_ids: vec!["vc-unit-1719-lsp-lang".to_string()],
            lsp: LspConfig {
                binary: "vc-unit-1719-lsp-bin".to_string(),
                install_linux: "npm install -g vc-unit-1719-lsp-bin".to_string(),
                install_macos: "npm install -g vc-unit-1719-lsp-bin".to_string(),
                install_windows: "npm install -g vc-unit-1719-lsp-bin".to_string(),
                dependencies: vec!["npm".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }]);

        e.ext_install_from_registry_with_runtime_check(ext_name, |_| false);

        assert!(
            e.pending_terminal_command.is_none(),
            "a missing declared dependency must never dispatch an install; got: {:?}",
            e.pending_terminal_command
        );
        assert!(
            e.message.contains("npm"),
            "status line must name the missing dependency; got: {}",
            e.message
        );
    }

    /// Same contract as above, but for the DAP leg's *manifest-declared*
    /// `install_*` command, using the new `dap.dependencies` field (#1719 —
    /// this field didn't exist before this issue).
    #[test]
    fn dap_manifest_install_blocked_when_declared_dependency_missing() {
        use crate::core::extensions::{DapConfig, ExtensionManifest};

        let mut e = Engine::new();
        let ext_name = "vc-unit-1719-dap-missing-dep";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1719 DAP missing-dep test".to_string(),
            dap: DapConfig {
                adapter: "vc-unit-1719-dap-adapter".to_string(),
                binary: "vc-unit-1719-dap-bin".to_string(),
                install_linux: "gem install vc-unit-1719-dap-bin".to_string(),
                install_macos: "gem install vc-unit-1719-dap-bin".to_string(),
                install_windows: "gem install vc-unit-1719-dap-bin".to_string(),
                dependencies: vec!["gem".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }]);

        e.ext_install_from_registry_with_runtime_check(ext_name, |_| false);

        assert!(
            e.pending_terminal_command.is_none(),
            "a missing declared DAP dependency must never dispatch an install; got: {:?}",
            e.pending_terminal_command
        );
        assert!(
            e.message.contains("gem"),
            "status line must name the missing DAP dependency; got: {}",
            e.message
        );
    }

    /// #1719: the built-in `delve` adapter's installer runs `go install
    /// github.com/go-delve/delve/cmd/dlv@latest` — nothing declared `go` as
    /// a prerequisite anywhere before this issue (`dap.dependencies` did
    /// not even exist), so a machine without `go` saw an opaque `command
    /// not found: go` inside the install pane rather than an upfront
    /// message. This drives the real merge of `dap_manager::adapter_
    /// dependencies` into `ext_install_from_registry_with_runtime_check`'s
    /// built-in-installer branch, not just the pure function on its own.
    ///
    /// Verified RED against the pre-#1719 code (the built-in branch calling
    /// `install_cmd_for_adapter` unconditionally): `pending_terminal_
    /// command` ends up `Some("... go install ...")` and `e.message` never
    /// mentions `go` as a missing prerequisite.
    #[test]
    fn dap_builtin_delve_install_blocked_when_go_missing() {
        use crate::core::extensions::{DapConfig, ExtensionManifest};

        let mut e = Engine::new();
        let ext_name = "vc-unit-1719-delve-missing-go";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1719 delve missing-go test".to_string(),
            dap: DapConfig {
                adapter: "delve".to_string(),
                // Deliberately not the literal `dlv` binary name — a dev
                // machine that has ever run `go install .../dlv` has it
                // sitting in `~/go/bin`, which `resolve_command` probes
                // regardless of PATH, short-circuiting the "already
                // resolvable" check before this test's prerequisite check
                // ever runs. See the sibling codelldb test's comment.
                binary: "vc-unit-1719-nonexistent-dlv".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);

        // Neither the adapter binary nor `go` is on PATH — the runtime
        // predicate below always returns false, so this is deterministic
        // regardless of what's on the machine running the suite.
        e.ext_install_from_registry_with_runtime_check(ext_name, |_| false);

        assert!(
            e.pending_terminal_command.is_none(),
            "delve's install must never run without `go` present; got: {:?}",
            e.pending_terminal_command
        );
        assert!(
            e.message.contains("go"),
            "status line must name `go` as the missing prerequisite; got: {}",
            e.message
        );
    }

    /// #1719: the built-in `codelldb` adapter's Unix installer needs
    /// `curl` and `unzip`; pin that both are checked (not just the first
    /// one found).
    #[test]
    fn dap_builtin_codelldb_install_blocked_when_curl_and_unzip_missing() {
        use crate::core::extensions::{DapConfig, ExtensionManifest};

        let mut e = Engine::new();
        let ext_name = "vc-unit-1719-codelldb-missing-tools";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1719 codelldb missing-tools test".to_string(),
            dap: DapConfig {
                adapter: "codelldb".to_string(),
                // Deliberately not the literal `codelldb` binary name —
                // this dev machine (and maybe the CI runner) may have a
                // real one on PATH already, which would short-circuit
                // `ext_install_from_registry_with_runtime_check`'s
                // "already resolvable" check before the prerequisite
                // check this test targets ever runs.
                binary: "vc-unit-1719-nonexistent-codelldb".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);

        e.ext_install_from_registry_with_runtime_check(ext_name, |_| false);

        assert!(
            e.pending_terminal_command.is_none(),
            "codelldb's install must never run without curl/unzip present; got: {:?}",
            e.pending_terminal_command
        );
        #[cfg(not(target_os = "windows"))]
        {
            assert!(
                e.message.contains("curl") && e.message.contains("unzip"),
                "status line must name both missing prerequisites; got: {}",
                e.message
            );
        }
    }

    /// A manifest that already declares its *own* `install_*` for a
    /// built-in adapter name (`codelldb`) must be checked against its own
    /// `dap.dependencies`, not the built-in curl/unzip list — the built-in
    /// list only applies when nothing overrides the install command.
    #[test]
    fn dap_manifest_override_of_builtin_adapter_uses_its_own_dependencies() {
        use crate::core::extensions::{DapConfig, ExtensionManifest};

        let mut e = Engine::new();
        let ext_name = "vc-unit-1719-codelldb-override";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1719 codelldb override test".to_string(),
            dap: DapConfig {
                adapter: "codelldb".to_string(),
                // See the sibling test above for why this isn't the
                // literal `codelldb` binary name.
                binary: "vc-unit-1719-nonexistent-codelldb".to_string(),
                install_linux: "pip install codelldb-shim".to_string(),
                install_macos: "pip install codelldb-shim".to_string(),
                install_windows: "pip install codelldb-shim".to_string(),
                dependencies: vec!["pip".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }]);

        // `pip` is declared missing; `curl`/`unzip` (the built-in codelldb
        // prerequisites) are irrelevant here since this manifest overrides
        // the install command entirely.
        e.ext_install_from_registry_with_runtime_check(ext_name, |dep| dep != "pip");

        assert!(
            e.pending_terminal_command.is_none(),
            "the overriding manifest's own missing dependency must still block install; got: {:?}",
            e.pending_terminal_command
        );
        assert!(
            e.message.contains("pip"),
            "status line must name the manifest's own declared dependency, not the \
             built-in codelldb list; got: {}",
            e.message
        );
    }

    // ── #1761 review: `ext_refresh_inner`'s dedupe-vs-quiet interaction ──

    /// An explicit, user-requested refresh (`ext_refresh`, non-quiet) that
    /// arrives while a quiet (startup) fetch is already in flight must
    /// upgrade the shared flag, not silently inherit the in-flight fetch's
    /// quiet policy — otherwise the user's `r` keypress in the Extensions
    /// panel produces no feedback at all when the fetch eventually lands
    /// (the exact #1761 review finding this covers).
    #[test]
    fn explicit_refresh_upgrades_an_in_flight_quiet_fetch() {
        let mut e = Engine::new_for_test();
        // Simulate a startup quiet fetch already in flight — deterministic,
        // no real channel/thread needed to exercise the dedupe branch.
        e.ext_registry_fetching = true;
        e.ext_registry_quiet = true;

        e.ext_refresh(); // non-quiet, explicit call

        assert!(
            e.ext_registry_fetching,
            "dedupe must not spawn a second fetch — the in-flight one stays armed"
        );
        assert!(
            !e.ext_registry_quiet,
            "an explicit (non-quiet) refresh that dedupes against an \
             in-flight quiet fetch must upgrade the flag so the fetch's \
             eventual completion still reports its result (#1761)"
        );
    }

    /// The mirror case: a *quiet* call (startup's own, or a second one
    /// somehow fired) that dedupes against an already-in-flight *explicit*
    /// fetch must not downgrade it back to quiet — the user who triggered
    /// the in-flight fetch is still owed its message.
    #[test]
    fn quiet_refresh_does_not_downgrade_an_in_flight_explicit_fetch() {
        let mut e = Engine::new_for_test();
        e.ext_registry_fetching = true;
        e.ext_registry_quiet = false;

        e.ext_refresh_quiet();

        assert!(
            e.ext_registry_fetching,
            "dedupe must not spawn a second fetch"
        );
        assert!(
            !e.ext_registry_quiet,
            "a quiet call deduping against an in-flight explicit fetch must \
             not downgrade it back to quiet (#1761)"
        );
    }

    // ── #1761 review: `poll_ext_registry`'s redraw verdict while the
    //    Extensions panel is open during a quiet fetch ──

    /// Drive `poll_ext_registry` directly (no real background thread) to
    /// pin down its return value for the `(quiet, panel_open)` combinations
    /// the #1761 review found incompletely handled: the panel being open
    /// while a *quiet* fetch lands must still request a redraw (the panel's
    /// list needs the fetch result just populated), even though the common
    /// quiet/closed-panel startup case must not.
    fn poll_with(quiet: bool, panel_open: bool) -> bool {
        // `poll_ext_registry`'s success branch calls `registry::save_cache`
        // unconditionally, which resolves through
        // `paths::vimcode_config_dir()`. Without a `TestHomeGuard` held
        // across that call, this test would write `[]` over the *real*
        // `~/.config/vimcode/registry_cache.json` on whatever machine runs
        // the suite — and a truncated cache is sticky, because `load_cache`
        // then returns `Some([])`, which makes `sidebar.rs`'s
        // `ext_registry.is_none()` guard false and leaves the developer's
        // Extensions panel painting empty until they refresh by hand.
        // Thread-local override, not `set_var("HOME", …)` — see
        // `core::paths::TEST_HOME_OVERRIDE` (#1741 review, #1761 review).
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_1761_poll_home_q{quiet}_p{panel_open}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let home_guard = crate::core::paths::set_test_home(&home);

        let mut e = Engine::new_for_test();
        if panel_open {
            e.app_shell
                .show_panel(&quadraui::WidgetId::new(PANEL_EXTENSIONS));
        }
        assert_eq!(
            e.active_panel_is(PANEL_EXTENSIONS),
            panel_open,
            "test setup: activating the Extensions panel must actually take \
             (PANEL_EXTENSIONS must be registered in the test engine's \
             app_shell panel list)"
        );
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Some(Vec::new())).unwrap();
        e.ext_registry_rx = Some(rx);
        e.ext_registry_fetching = true;
        e.ext_registry_quiet = quiet;
        // Bind, don't tail-return: the guard must still be alive while
        // `poll_ext_registry` runs `save_cache` (a tail expression would
        // also be fine today, but binding makes the ordering explicit and
        // lets the cache assertion below run under the override).
        let redraw = e.poll_ext_registry();
        assert!(
            home.join(".config/vimcode/registry_cache.json").exists(),
            "the fetch's cache write must land under the test home override, \
             not the real ~/.config/vimcode (#1761 review)"
        );
        drop(home_guard);
        let _ = std::fs::remove_dir_all(&home);
        redraw
    }

    #[test]
    fn quiet_fetch_with_panel_closed_reports_no_redraw_needed() {
        // The common cold-launch case this issue is about: nothing visible
        // reads `ext_registry` while the panel is closed, so no redraw.
        assert!(!poll_with(true, false));
    }

    #[test]
    fn quiet_fetch_with_panel_open_still_requests_a_redraw() {
        // #1761 review: opening the Extensions panel while a quiet fetch is
        // in flight does not re-arm its own refresh (`set_panel_focus`'s
        // `PANEL_EXTENSIONS` arm only calls `ext_refresh` when nothing is
        // already fetching), so the panel relies entirely on *this* fetch's
        // completion to paint anything. Suppressing the redraw here would
        // leave the open panel showing an empty/stale list.
        assert!(poll_with(true, true));
    }

    #[test]
    fn explicit_fetch_always_requests_a_redraw_regardless_of_panel_state() {
        assert!(poll_with(false, false));
        assert!(poll_with(false, true));
    }

    /// #1807: `ext_install_from_registry` must refuse an extension whose
    /// `requires_vimcode` constraint isn't met by the running vimcode —
    /// engine-internal-state half of the acceptance bar; the driver-tier
    /// black-box twin (asserting on `driver.screen()`) is
    /// `tui_main::app_on_tui_tests::tests::issue_1807_requires_vimcode_gates_install::
    /// far_future_requires_vimcode_blocks_install_with_clear_message`.
    #[test]
    fn install_refused_when_requires_vimcode_is_unmet() {
        use crate::core::extensions::ExtensionManifest;

        let mut e = Engine::new();
        let ext_name = "vc-unit-1807-future-ext";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1807 Future Extension".to_string(),
            requires_vimcode: Some(">=9999.0.0".to_string()),
            scripts: vec!["init.lua".to_string()],
            ..Default::default()
        }]);

        e.ext_install_from_registry(ext_name);

        assert!(
            e.message.contains("Cannot install") && e.message.contains("requires vimcode"),
            "got: {}",
            e.message
        );
        assert!(
            !e.extension_state.is_installed(ext_name),
            "a refused install must never mark the extension installed"
        );
    }

    /// #1807: absent `requires_vimcode` (the pre-#1807 default) must never
    /// trip the new gate — the "If the field is missing, behave as today"
    /// acceptance criterion.
    ///
    /// This install runs to completion (`mark_installed_version` +
    /// `.save()`, then would reach `plugin_init` in a real session), so it
    /// uses `set_test_home` to keep that persistence under a throwaway
    /// directory instead of the developer's real `~/.config/vimcode`
    /// (review finding on this issue's round 1).
    #[test]
    fn install_not_refused_when_requires_vimcode_is_absent() {
        use crate::core::extensions::ExtensionManifest;

        let unique = format!(
            "vimcode_test_1807_lsp_ops_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let home = std::env::temp_dir().join(&unique);
        std::fs::create_dir_all(&home).unwrap();
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut e = Engine::new();
        let ext_name = "vc-unit-1807-legacy-ext";
        e.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.to_string(),
            display_name: "1807 Legacy Extension".to_string(),
            ..Default::default()
        }]);

        e.ext_install_from_registry(ext_name);

        assert!(
            !e.message.contains("Cannot install"),
            "an extension with no requires_vimcode field must never be \
             refused by the new gate; got: {}",
            e.message
        );

        drop(_home_guard);
        let _ = std::fs::remove_dir_all(&home);
    }
}
