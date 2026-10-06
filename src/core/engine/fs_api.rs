use super::*;

// ─── `vimcode.fs.walk` / `vimcode.fs.grep` (#1806) ─────────────────────────
//
// The native Files/Grep pickers (`src/core/engine/picker.rs`) and
// `:grep`/project-search (`src/core/engine/search.rs`) all walk the
// filesystem in-process with the `ignore` crate — no `rg`/`fd` needed. A
// Lua extension could previously only reach the filesystem through
// `vimcode.loop.spawn`, so a finder extension (the #1212 epic's
// telescope-style replacement for the built-in pickers) would have
// depended on `rg`/`fd` being installed — a regression on a fresh install
// and on Windows. `vimcode.fs.walk`/`vimcode.fs.grep` expose the same
// in-process walker to Lua instead, built on `project_search::
// walk_project_streaming`/`grep_project_streaming` so there is exactly one
// implementation of "walk the project respecting gitignore/exclude rules",
// not two.
//
// Shape mirrors `vimcode.loop.spawn`/`vimcode.http.request` exactly: a
// background thread does the (possibly slow) work and streams results
// through an `mpsc` channel, polled from `poll_idle` via
// `Engine::poll_plugin_fs` and delivered through the same `PluginCallContext`/
// `EngineLoan` machinery every other stored callback uses. The difference
// from `vimcode.http.request` is cardinality: an HTTP request delivers
// exactly one result, a walk/grep delivers zero or more batches followed by
// exactly one `on_done` (or, if cancelled, nothing further at all — a
// finder re-queries on every keystroke, so cancelling the previous query
// must stop delivery promptly, not just stop new work from starting).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use project_search::{FsGrepMatch, FsGrepOptions, FsWalkOptions};

/// Per-handle, per-`poll_idle`-tick cap on how many queued `FsEvent`s
/// [`Engine::poll_plugin_fs`] drains before moving to the next handle (or
/// returning). Draining *all* currently-queued events in one tick (rather
/// than one per tick) keeps a fast producer's batches from visibly lagging
/// a frame behind — but with no cap at all, a producer thread that
/// consistently outruns the consumer (plausible: the walk is a tight
/// syscall loop, the consumer crosses into Lua and runs plugin code per
/// batch) means `try_recv` keeps succeeding and this tick never returns
/// until the whole walk finishes, stalling the UI thread for the walk's
/// full duration. The cap bounds that worst case while still draining
/// everything queued *so far* on every ordinary tick (#1806 review).
const FS_POLL_MAX_EVENTS_PER_TICK: usize = 32;

/// One streamed event from a `vimcode.fs.walk`/`vimcode.fs.grep` background
/// thread. `Done` is sent exactly once, after the last batch — never sent at
/// all if the walk was cancelled first (mirrors the doc comment above: no
/// callback fires after `cancel()`).
pub(crate) enum FsEvent {
    WalkBatch(Vec<std::path::PathBuf>),
    GrepBatch(Vec<FsGrepMatch>),
    Done,
}

/// Live state for one `vimcode.fs.walk`/`vimcode.fs.grep` handle (#1806):
/// the cancellation flag shared with the background thread (checked between
/// every entry/batch — see `project_search::walk_project_streaming`/
/// `grep_project_streaming`) and the batch/done event stream. Mirrors
/// `execute::PluginHttpHandle` minus the child process (this background
/// thread is a plain walk, not a spawned process).
pub(crate) struct PluginFsHandle {
    /// Weak so an unloaded owning plugin is detectable the same way
    /// `PluginSpawnHandle::manager`/`PluginHttpHandle::manager` are.
    pub(crate) manager: std::rc::Weak<plugin::PluginManager>,
    pub(crate) cancelled: Arc<AtomicBool>,
    pub(crate) rx: Receiver<FsEvent>,
}

impl Engine {
    /// Resolve `vimcode.fs.walk`/`vimcode.fs.grep`'s `root` argument against
    /// `self.cwd`, the same way `Engine::picker_load_preview` resolves a
    /// plugin-picker item's relative file path: an empty string means "the
    /// workspace root", and `cwd.join(root)` is a no-op when `root` is
    /// already absolute (`PathBuf::join` replaces the base entirely for an
    /// absolute right-hand side).
    ///
    /// Deliberately unconfined: an absolute `root` (e.g. `/`) or a relative
    /// one containing `..` is not rejected or normalised, so a walk/grep can
    /// read anywhere the process can, not just under the workspace. Given
    /// `vimcode.loop.spawn` already grants arbitrary process execution to
    /// any loaded plugin, this doesn't change the trust model — plugins are
    /// unrestricted by design (see the module doc at the top of
    /// `plugin.rs`) — but it's worth stating explicitly so a future reader
    /// doesn't assume a confinement that was never implemented (#1806
    /// review).
    fn resolve_fs_root(&self, root: &str) -> std::path::PathBuf {
        if root.is_empty() {
            self.cwd.clone()
        } else {
            self.cwd.join(root)
        }
    }

    /// `vimcode.fs.walk(root, opts, on_batch, on_done)`: list files under
    /// `root` on a background thread, the same gitignore/exclude-aware walk
    /// `Engine::picker_populate_files` uses — including that picker's
    /// `Settings::explorer_exclude` pruning, filled into `opts` here since
    /// Lua can't set it directly — streaming paths to `on_batch` in batches.
    /// Returns the handle id, or `Err` with a human-readable reason if there
    /// is no live plugin manager or `opts`'s glob patterns don't parse (the
    /// Lua binding surfaces it as a runtime error, matching `vimcode.loop.
    /// spawn`'s "failed to start" convention — but with the actual
    /// `SearchError` message included rather than a generic one, since the
    /// main consumer of this API compiles user-typed glob patterns on every
    /// call, #1806 review).
    pub(crate) fn plugin_api_fs_walk(
        &mut self,
        root: String,
        mut opts: FsWalkOptions,
        on_batch: mlua::RegistryKey,
        on_done: mlua::RegistryKey,
    ) -> Result<i64, String> {
        let pm = self
            .plugin_manager
            .clone()
            .ok_or("no live plugin manager")?;
        let root = self.resolve_fs_root(&root);
        opts.explorer_exclude = self.settings.explorer_exclude.clone();
        // Register the callbacks *before* validating the glob patterns, not
        // after — the reverse order drops `on_batch`/`on_done`'s
        // `LuaRegistryKey`s on the ground on a bad-glob call, since nothing
        // ever reaches `remove_fs_callbacks` to reclaim their registry slot
        // (same mistake `plugin_api_http_request`'s doc comment calls out
        // for #1632; #1806 review).
        let id = pm.register_fs_callbacks(on_batch, on_done);
        let overrides = match project_search::validate_fs_walk_options(&root, &opts) {
            Ok(overrides) => overrides,
            Err(e) => {
                pm.remove_fs_callbacks(id);
                return Err(e.0);
            }
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread_cancelled = cancelled.clone();
        // `root` is consumed by `walk_project_streaming` below; keep a
        // separate clone to relativize each batch's paths against before
        // they're sent — see `PluginManager::call_fs_walk_batch`'s doc
        // comment for why the paths must be root-relative.
        let display_root = root.clone();
        std::thread::spawn(move || {
            project_search::walk_project_streaming(
                &root,
                &opts,
                overrides,
                &thread_cancelled,
                |batch| {
                    let batch: Vec<std::path::PathBuf> = batch
                        .into_iter()
                        .map(|p| match p.strip_prefix(&display_root) {
                            Ok(rel) => rel.to_path_buf(),
                            // Can't happen for a `root`-rooted walk (every
                            // entry `ignore::WalkBuilder` yields is under the
                            // root it was built from) — kept as a defensive
                            // fallback rather than silently dropping the
                            // entry.
                            Err(_) => p,
                        })
                        .collect();
                    let _ = tx.send(FsEvent::WalkBatch(batch));
                },
            );
            if !thread_cancelled.load(Ordering::Relaxed) {
                let _ = tx.send(FsEvent::Done);
            }
        });
        self.plugin_fs_ops.insert(
            id,
            PluginFsHandle {
                manager: std::rc::Rc::downgrade(&pm),
                cancelled,
                rx,
            },
        );
        Ok(id as i64)
    }

    /// `vimcode.fs.grep(root, pattern, opts, on_batch, on_done)`: regex or
    /// literal content search under `root` on a background thread, streaming
    /// `{path, line, col, text}` matches to `on_batch` in batches. Returns
    /// `Err` with a human-readable reason (surfaced by the Lua binding as a
    /// runtime error) if there is no live plugin manager, `pattern` doesn't
    /// compile as a regex (when `opts.use_regex`), or `opts`'s glob patterns
    /// don't parse — see `plugin_api_fs_walk`'s doc comment for why the
    /// actual `SearchError` text is surfaced rather than a generic message
    /// (#1806 review).
    pub(crate) fn plugin_api_fs_grep(
        &mut self,
        root: String,
        pattern: String,
        opts: FsGrepOptions,
        on_batch: mlua::RegistryKey,
        on_done: mlua::RegistryKey,
    ) -> Result<i64, String> {
        let pm = self
            .plugin_manager
            .clone()
            .ok_or("no live plugin manager")?;
        let root = self.resolve_fs_root(&root);
        let max_results = opts.max_results;
        // Register before validating — see `plugin_api_fs_walk`'s comment on
        // the same ordering (#1806 review): an invalid regex/glob must still
        // reclaim `on_batch`/`on_done`'s registry slots via
        // `remove_fs_callbacks` rather than dropping the keys unregistered.
        let id = pm.register_fs_callbacks(on_batch, on_done);
        let (re, overrides) = match project_search::validate_fs_grep_options(&root, &pattern, &opts)
        {
            Ok(pair) => pair,
            Err(e) => {
                pm.remove_fs_callbacks(id);
                return Err(e.0);
            }
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let thread_cancelled = cancelled.clone();
        std::thread::spawn(move || {
            project_search::grep_project_streaming(
                &root,
                &re,
                overrides,
                max_results,
                &thread_cancelled,
                |batch| {
                    let _ = tx.send(FsEvent::GrepBatch(batch));
                },
            );
            if !thread_cancelled.load(Ordering::Relaxed) {
                let _ = tx.send(FsEvent::Done);
            }
        });
        self.plugin_fs_ops.insert(
            id,
            PluginFsHandle {
                manager: std::rc::Rc::downgrade(&pm),
                cancelled,
                rx,
            },
        );
        Ok(id as i64)
    }

    /// `vimcode.fs.walk(...):cancel()` / `vimcode.fs.grep(...):cancel()` —
    /// stop the background walk promptly (it's checked between every
    /// entry/batch) and drop the handle immediately. No `on_batch`/`on_done`
    /// fires for a cancelled handle, whether or not the background thread
    /// had already queued more events by the time this runs — same contract
    /// as `vimcode.http.request(...):cancel()`.
    pub(crate) fn plugin_api_fs_cancel(&mut self, id: i64) -> bool {
        if id < 0 {
            return false;
        }
        let id = id as u64;
        let Some(handle) = self.plugin_fs_ops.remove(&id) else {
            return false;
        };
        handle.cancelled.store(true, Ordering::Relaxed);
        if let Some(pm) = self.plugin_manager.clone() {
            pm.remove_fs_callbacks(id);
        }
        true
    }

    /// Deliver every pending `vimcode.fs.walk`/`vimcode.fs.grep` batch (and
    /// any `on_done` that has arrived), through the plugin dispatch loan.
    /// Called from `poll_idle`.
    ///
    /// Drains up to [`FS_POLL_MAX_EVENTS_PER_TICK`] events currently queued
    /// for each handle in one tick (rather than one event per tick) so a
    /// fast producer's batches don't lag behind — a finder repainting its
    /// list one batch per frame while several are already buffered would
    /// look stalled for no reason. Anything left over after the cap stays
    /// queued in the handle's `mpsc::Receiver` for the next tick rather than
    /// being dropped.
    ///
    /// A handle whose owning plugin manager was unloaded is cancelled and
    /// dropped here instead of having its callbacks invoked — same rule as
    /// `Self::poll_plugin_http`.
    ///
    /// Returns `true` if any callback fired (caller should redraw).
    pub fn poll_plugin_fs(&mut self) -> bool {
        if self.plugin_fs_ops.is_empty() {
            return false;
        }
        let ids: Vec<u64> = self.plugin_fs_ops.keys().copied().collect();
        let mut redraw = false;
        for id in ids {
            let alive = self
                .plugin_fs_ops
                .get(&id)
                .map(|h| h.manager.upgrade().is_some())
                .unwrap_or(false);
            if !alive {
                if let Some(handle) = self.plugin_fs_ops.remove(&id) {
                    handle.cancelled.store(true, Ordering::Relaxed);
                }
                continue;
            }
            let mut drained = 0usize;
            while drained < FS_POLL_MAX_EVENTS_PER_TICK {
                let Some(ev) = self
                    .plugin_fs_ops
                    .get(&id)
                    .and_then(|h| h.rx.try_recv().ok())
                else {
                    break;
                };
                drained += 1;
                match ev {
                    FsEvent::WalkBatch(paths) => {
                        let ctx = self.make_plugin_ctx(true);
                        let applied = self
                            .with_plugin_dispatch(move |pm| pm.call_fs_walk_batch(id, paths, ctx));
                        if let Some(ctx) = applied {
                            self.apply_plugin_ctx(ctx);
                            redraw = true;
                        }
                    }
                    FsEvent::GrepBatch(matches) => {
                        let ctx = self.make_plugin_ctx(true);
                        let applied = self.with_plugin_dispatch(move |pm| {
                            pm.call_fs_grep_batch(id, matches, ctx)
                        });
                        if let Some(ctx) = applied {
                            self.apply_plugin_ctx(ctx);
                            redraw = true;
                        }
                    }
                    FsEvent::Done => {
                        self.plugin_fs_ops.remove(&id);
                        let ctx = self.make_plugin_ctx(true);
                        let applied = self.with_plugin_dispatch(move |pm| pm.call_fs_done(id, ctx));
                        if let Some(ctx) = applied {
                            self.apply_plugin_ctx(ctx);
                            redraw = true;
                        }
                        if let Some(pm) = self.plugin_manager.clone() {
                            pm.remove_fs_callbacks(id);
                        }
                        break;
                    }
                }
            }
        }
        redraw
    }
}
