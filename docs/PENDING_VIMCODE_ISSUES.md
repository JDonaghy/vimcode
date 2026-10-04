# Pending vimcode issues — drafted, not yet filed

Vimcode worker sessions in this harness are `git`-only (no `gh` access); filing
GitHub issues, including on this repo (`JDonaghy/vimcode`) itself, is a
coordinator/human action. This file is `docs/PENDING_QUADRAUI_ISSUES.md`'s
sibling for gaps found *in this repo*, for the same reason that one exists: a
comment naming a known-but-unfixed deviation is an unfiled issue, and grep
will not find it for you.

**Coordinator/human action:** file each entry below verbatim on
`JDonaghy/vimcode`, then delete its entry here and update the citing
`KNOWN_DEVIATIONS`/code comment with the real issue number.

---

## Explorer has no `Outline`/`Timeline` sibling sections (#1693 ask item 2) — a new engine feature, not a backend wiring gap

**Title:** `Engine::explorer_tree` is a bare `quadraui::TreeController` — one
tree, no sibling sections. VS Code's Explorer view has three collapsible
sections stacked in one sidebar panel: the file tree, `Outline` (the active
file's document symbols), and `Timeline` (the active file's git history).
Neither `Outline` nor `Timeline` exists anywhere in vimcode today, on *any*
backend — this is net-new scope, not a Win-GUI-specific gap (#1693's own
"Scope" section already says as much: "Sidebar structure... built
backend-neutrally in vimcode").

**Body:**

Investigated while working #1693's narrower ask item 3 (the Explorer
header's view-actions toolbar, landed separately in the same PR this entry
ships alongside). Checked whether the infrastructure for multiple
collapsible sections in one sidebar panel already exists before concluding
this needs new code — it does: `quadraui::SidebarSystem` (`compose/
sidebar_system.rs`) already supports exactly this shape, and four other
vimcode panels already use it (`search_sidebar_system`, `sc_sidebar_system`,
`dap_sidebar_system`, `ext_sidebar_system` — all `Engine` fields). The
Explorer is the one built-in sidebar panel that still drives a bare
`TreeController` instead — `explorer_ops.rs`'s `explorer_reveal_path` doc
names this directly ("the explorer drives a bare `TreeController`... and is
not a `SidebarSystem` section at all") as blocking a *different*, smaller
piece of promotion work (#659), for an unrelated reason (no `TreeController::
reveal` upstream yet).

Migrating Explorer onto `SidebarSystem` is necessary but not sufficient for
this ask: it would give the panel multiple named, collapsible sections (the
`SidebarSectionDef` shape `Outline`/`Timeline` need), but `Outline`'s and
`Timeline`'s *row data* needs new engine-level sourcing that does not exist
today:
- `Outline`: the active file's document symbols, grouped/ordered the way
  `lsp.rs`'s own doc comment already describes VS Code's Outline sort order
  (category-grouped: classes/structs, then functions, then variables) —
  `Engine::lsp_request_document_symbols`-shaped data exists for the
  "Go to Symbol in Editor (Outline)" picker command already; reusing it as
  a *sidebar tree*, kept live as the cursor moves and the active buffer
  changes, is new.
- `Timeline`: the active file's git history (one row per commit touching
  it) — `git.rs` has `blame_file_structured`/commit-log primitives already,
  but nothing today builds a Timeline-shaped row list from them.

**Ask:** Scope as its own issue (or small epic): (1) migrate
`Engine::explorer_tree` onto a `SidebarSystem` with a `Files` section
wrapping today's tree unchanged, verified by the existing Explorer driver
tests staying green with no behaviour change; (2) add an `Outline` section
sourced from LSP document symbols, live-updated on cursor move / buffer
switch; (3) add a `Timeline` section sourced from git log for the active
file. Each is independently shippable and independently testable via
`GtkDriver`/`TuiDriver` (`screen_contains("Outline")` + its own collapsible
chevron, per #1693's acceptance bar).

**Test:** None added this round — this entry is scope description, not a
landed fix or a reproduced bug.

**Blocks:** `JDonaghy/vimcode#1693` stays open behind this — ask items 1
and 3 of that issue are addressed in the PR this entry ships alongside; ask
item 2 (this entry) and ask item 4 (indent guides, drafted in
`docs/PENDING_QUADRAUI_ISSUES.md`) are not.

---

## #1719 remainder — no minimum-version checking, and the headless `--ext-install` entry point doesn't speak `initialize` yet

**Title:** `#1719`'s "detect before install" half landed (LSP/DAP
dependency checks run before any install is dispatched, `dap.dependencies`
exists, the built-in codelldb/debugpy/delve/netcoredbg adapters declare
their own curl/unzip/tar/python3/go prerequisites, and `vimcode --ext-install
<name> --json` reports a missing-prerequisite verdict headlessly without
running a doomed install — all RED-verified against unfixed code). Two
pieces of the issue's own "Fix" list are explicitly **not** done:

1. **No minimum-version checking.** `dependencies`/`adapter_dependencies`
   are presence-only (`resolve_command(dep).is_some()`) — a *present but
   too old* `go` (the issue's own example: Ubuntu 24.04's `golang-go` is
   1.22, `gopls@latest` wants materially newer) still passes detection and
   fails later with the tool's own error, exactly as the issue describes.
   The `go` hint itself was changed to `snap install go --classic`
   (tracks current stable, sidesteps `GOTOOLCHAIN=local` from a distro
   package) as a mitigation, but that's an install-string fix, not a
   version *check*. Adding real version checking needs a manifest schema
   decision (a `min_versions` table, or `dependencies = ["go>=1.21"]`
   inline syntax) plus a per-tool "how do I ask this binary its version"
   probe (`go version`, `dotnet --version`, etc. all format differently) —
   scoped as its own issue rather than guessed at here.
2. **The headless entry point doesn't verify `initialize`.** `--ext-install`
   checks prerequisites, reports the instruction, and — once every
   prerequisite is present — runs the resolved install command and reports
   its exit status. It does not then speak LSP/DAP `initialize` against the
   freshly installed server/adapter to confirm "working" (the issue's
   contract item 3). `vimcode-ext#17`'s own CI matrix has the per-language
   client fixtures to drive that handshake and was always going to need
   them regardless of what this entry point does; wiring a generic
   initialize-probe into `vimcode --ext-install` itself (rather than
   leaving it to the external matrix) is additional scope worth deciding
   deliberately, not bundling into this fix silently.

**Ask:** File both as their own issues (or one combined "harden #1719"
follow-up) once `#1719` itself is reviewed, so the two known gaps don't
quietly become "the bug was fixed" in release notes — see CLAUDE.md's
"Never describe a `KNOWN_BUGS`-gated issue as fixed" rule, which applies in
spirit here even though this isn't a `KNOWN_BUGS` entry: the PR landing
this says `ISSUE_RESOLUTION: partial` for exactly this reason.

**Test:** `every_prereq_install_entry_is_runnable_on_every_platform` and
`prereq_install_cmd_covers_the_1719_additions` (`src/core/extensions.rs`),
`adapter_dependencies_*` (`src/core/dap_manager.rs`),
`lsp_legacy_install_blocked_when_declared_dependency_missing` /
`dap_manifest_install_blocked_when_declared_dependency_missing` /
`dap_builtin_delve_install_blocked_when_go_missing` /
`dap_builtin_codelldb_install_blocked_when_curl_and_unzip_missing` /
`dap_manifest_override_of_builtin_adapter_uses_its_own_dependencies`
(`src/core/engine/lsp_ops.rs`), `ext_install_flag_and_value_parse_and_json_
is_recognised` (`src/main.rs`), and the black-box
`ext_install_reports_missing_npm_and_runs_no_install_without_prerequisite`
(`tests/ext_install_cli.rs`, spawns the real binary) all landed with this
entry and were RED-verified against the pre-fix code.

---
