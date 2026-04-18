# Cross-Platform UI Crate — Design Sketch

**Status:** Draft for discussion. No code yet.
**Audience:** The author and collaborators deciding whether this direction is sound before committing to a refactor.

---

## 1. Vision

Extract a **cross-platform UI crate** from vimcode that lets developers build rich, fast, keyboard-driven desktop + terminal applications from a single codebase.

Targets:
- **Windows** — native via Direct2D + DirectWrite (already working in `src/win_gui/`)
- **Linux** — native via GTK4 + Cairo + Pango (already working in `src/gtk/`)
- **macOS** — native via Core Graphics + Core Text (planned, issue #47)
- **TUI** — ratatui + crossterm (already working in `src/tui_main/`)

**Test app:** vimcode. It stresses the library: 20+ panels, complex text editing, drag-and-drop splits, tree views, forms, modals, command palette, dialogs.

**Ambition:** Someone should be able to write a SQL client, a k8s dashboard (Lens-alike), a log viewer, or a package manager UI on top of this crate and get all four backends for free. Apps should **feel native** (platform font, chrome, accent colour, menus, keybindings) without **being native** (no `gtk::TreeView`, no `NSOutlineView`, no Win32 Common Controls). The app writes once; the crate renders everywhere.

---

## 2. Non-goals

- **Not a general-purpose GUI framework** like GTK, Qt, Iced, Slint, or egui. We're biased toward keyboard-driven, productivity-class apps: IDEs, database tools, devops consoles. Apps with heavy custom drawing (games, video editors, Figma) are out of scope.
- **Not a web-tech shim** like Tauri or Electron. No WebView. No HTML/CSS. No JavaScript.
- **Not a pixel-perfect Figma-style renderer.** Widgets look *consistent* with each platform's conventions (font, accent, spacing, scrollbar style) but do not attempt to mimic `NSButton` or `ttk::Button` exactly.
- **Not retained-mode DOM.** No virtual DOM diffing. Full rebuild each frame — see §3.
- **Not an animation framework.** Transitions are out of scope v1. Fades and spinners can be added later as primitives.

---

## 3. Core architecture

### 3.1 Rendering model: build-and-draw each frame

Each frame:

1. App produces a **description tree** of the UI for the current state (pure function of app state).
2. Crate consumes the tree, walks it with the active backend, and renders.
3. Backend captures input (key, mouse) and returns a **list of events** to the app.
4. App mutates its state and loops.

This is the same loop vimcode already runs: `build_screen_layout()` is called every frame; backends just draw. We're generalizing it.

No diffing. No virtual DOM. No retained widget state managed by the crate. All state lives in the app; the crate is a pure view function with an input event channel.

**Why not retained-mode with diffing (React/Relm)?** Simpler mental model, fewer footguns (stale widget state, identity mismatches), faster to implement per-backend. vimcode already proves the rebuild-every-frame approach scales to dozens of panels.

**Exception — scroll and text input state.** Scroll offsets, cursor position in text inputs, and fuzzy-search query boxes are *primitive-owned*: the crate retains them to avoid forcing every app to thread them through. Primitives emit events when these change so the app can persist or react.

### 3.2 Event model

Each primitive has a typed event enum. Example:

```rust
pub enum TreeEvent {
    RowClicked { path: TreePath, modifiers: Modifiers },
    RowDoubleClicked { path: TreePath },
    RowExpanded { path: TreePath },
    RowCollapsed { path: TreePath },
    ContextMenuRequested { path: TreePath, screen_pos: Point },
    SelectionChanged { path: TreePath },
    KeyPressed { key: Key, handled: bool },
}
```

Backend produces events; the crate bubbles them up to the app wrapped in an `UiEvent` discriminator:

```rust
pub enum UiEvent {
    Tree(WidgetId, TreeEvent),
    Form(WidgetId, FormEvent),
    TabBar(WidgetId, TabEvent),
    StatusBar(WidgetId, StatusEvent),
    ActivityBar(ActivityEvent),
    Palette(PaletteEvent),
    Window(WindowEvent),
    // ...
}
```

`WidgetId` is a stable ID the app assigns when describing the tree. It's how the app routes events to the right handler.

### 3.3 Fully-drawn, not native-widget

Every widget is rendered by the backend's 2D drawing API. No `gtk::TreeView`. No `NSOutlineView`. No Win32 `EDIT`. Win-GUI has proven this works end-to-end: zero native widgets, everything rasterized via Direct2D + DirectWrite.

**Benefits:**
- Identical visual behaviour across platforms (no "it works on Windows, different on Linux")
- One code path for selection, scroll, keyboard nav — no per-platform widget API to wrangle
- Testable: ScreenLayout-style snapshots are platform-agnostic

**Costs (real, must be mitigated):**
- **Accessibility** — no free a11y tree. We must publish a parallel accessibility structure to UI Automation (Win), NSAccessibility (Mac), AT-SPI (Linux). This is a meaningful amount of work and is a v2 topic. Apps built on the crate that ignore a11y will not meet screen-reader expectations out of the box.
- **IME / Unicode composition** — Korean/Japanese/Chinese input requires platform IME integration. Each backend must implement IME client protocol on text-input primitives. Non-trivial.
- **Platform right-click menus, scroll gestures, kinetic scrolling** — we reimplement.
- **Drag-and-drop across apps** — platform DnD protocols need explicit handling per backend.

These costs are acceptable for v1 (accessibility parked until there's a real user need; IME is a v1.1 goal; DnD within the app only). They'd be dealbreakers for a mass-market app framework.

### 3.4 "Native feel" without native widgets

Each backend supplies a `PlatformStyle`:

- **Default font:** SF Pro Text (macOS), Segoe UI Variable (Windows 11), system sans-serif (Linux), terminal default (TUI)
- **Default mono font:** SF Mono, Cascadia Code, JetBrains Mono, terminal
- **Accent colour:** system accent on Windows/macOS; VSCode blue fallback on Linux/TUI
- **Scrollbar style:** auto-hide overlay on macOS/Win11; always-visible on Linux/TUI
- **Window chrome:** platform-provided (Win32 title bar, macOS unified title, GTK CSD)
- **Menu placement:** macOS global menu bar; in-window menu bar on Windows/Linux; `:command` mode in TUI
- **Modifier conventions:** `Cmd` on macOS is `Ctrl` on Win/Linux for common shortcuts. The crate exposes a `Accelerator` type that renders appropriately per platform.

Apps don't hardcode hex colours; they read from `theme.palette`. Apps don't hardcode "Ctrl+S"; they register `Accelerator::Save` which renders as `⌘S` on macOS and `Ctrl+S` elsewhere.

---

## 4. Primitive catalog

A primitive is a widget type the crate renders across all backends. Apps compose primitives into screens.

### 4.1 Layout & containers

| Primitive | Purpose |
|-----------|---------|
| `Window` | Top-level frame; holds menu bar, status bar, content. One per logical window. |
| `Split` | Recursive binary split with draggable divider. Horizontal or vertical. Ratio or fixed px. |
| `Tabs` | Container with tab strip + content area. Drag-to-reorder, drag-to-split, close buttons. |
| `Panel` | Fixed or resizable panel (sidebar, bottom drawer). Has header + body. |
| `Stack` | Vertical or horizontal stack with per-child sizing (fixed, fill, ratio). |
| `Modal` | Overlay with backdrop. Blocks input to parent. Used for dialogs. |

### 4.2 Navigation chrome

| Primitive | Purpose |
|-----------|---------|
| `MenuBar` | Top menu bar (in-window on Win/Linux, global on macOS). Declarative menu tree. |
| `ActivityBar` | Vertical icon strip (VSCode-style left rail). Selected state, click events, tooltips. |
| `TabBar` | Horizontal tab strip, usable standalone or inside `Tabs`. Hit regions pre-computed by crate. |
| `StatusBar` | Segmented bottom bar with clickable regions. Left/right segment lists. |
| `BreadcrumbBar` | Optional row above editor showing file path, symbols, etc. |

### 4.3 Content widgets

| Primitive | Purpose |
|-----------|---------|
| `TreeView` | Hierarchical rows, expand/collapse, icons, multi-selection, context menu. |
| `ListView` | Flat rows with optional section headers, selection, context menu. |
| `DataTable` | Grid with sortable column headers, row selection, cell editing. For SQL results, k8s lists. |
| `TextEditor` | Multi-cursor text editor with syntax highlighting, folds, gutter, virtual text. vimcode-class. |
| `TextDisplay` | Read-only rich/plain text view; used for markdown preview, LSP hover, log output. |
| `Form` | Vertical stack of labeled fields: `Toggle`, `TextInput`, `Dropdown`, `Slider`, `Button`, `ColorPicker`. |
| `SearchBox` | Text input with embedded search affordances (clear button, match count, up/down). |
| `ProgressBar` | Determinate or indeterminate. |
| `Spinner` | Indeterminate activity indicator. |

### 4.4 Transient / popup

| Primitive | Purpose |
|-----------|---------|
| `Palette` | Fuzzy-search list, anchored at top of window. Command palette, file picker, symbol picker. |
| `ContextMenu` | Right-click menu, anchored at cursor. Nested submenus. |
| `Completions` | Autocomplete popup, anchored to a caret position. |
| `Tooltip` | Hover or focus-triggered info bubble. |
| `Dialog` | Built on `Modal`; standard button layouts (OK/Cancel, Yes/No/Cancel, error/warn/info). |
| `Toast` | Transient notification, anchored to corner. |

### 4.5 Primitive data shape (example)

```rust
pub struct TreeView<'a> {
    pub id: WidgetId,
    pub rows: &'a [TreeRow],           // flat, pre-expanded
    pub selection: SelectionMode,      // Single, Multi, None
    pub selected: &'a [TreePath],
    pub scroll: ScrollState,           // crate-owned; use ScrollState::id(widget_id)
    pub context_menu: Option<MenuDescription>,
    pub style: TreeStyle,              // indent_px, icon_size, row_height
}

pub struct TreeRow {
    pub path: TreePath,                // [0, 3, 1] style
    pub indent: u16,
    pub icon: Option<Icon>,
    pub text: StyledText,              // supports spans + colour
    pub badge: Option<Badge>,          // right-aligned indicator (git status, count, etc.)
    pub is_expanded: Option<bool>,     // None = leaf
    pub decoration: Option<Decoration>,// italic, strikethrough, muted
}
```

Apps build `TreeRow` lists from their own state each frame. Scroll is primitive-owned: `ScrollState::id(widget_id)` tells the crate "this is the same scrollable region across frames; retain my scroll offset."

---

## 5. Sample app sketches

Purpose of this section: **stress-test the primitive set** against three concrete apps to check for gaps.

### 5.1 vimcode (current)

| Screen area | Primitive composition |
|-------------|----------------------|
| Main window | `Window { menu_bar, content, status_bar }` |
| Sidebar | `Panel` containing `ActivityBar` + body (swaps between `TreeView` for explorer, `TreeView` for git/debug/extensions, `Form` for settings, etc.) |
| Editor area | `Tabs` with `TextEditor` inside each tab; supports `Split` containers recursively |
| Status bar | `StatusBar { left_segments, right_segments }` |
| Find/replace | `Panel` (overlay) containing `SearchBox` + `Form` |
| Command palette | `Palette` |
| Dialogs | `Dialog` (modal) |
| Completions | `Completions` anchored to `TextEditor` caret |
| Hover | `Tooltip` anchored to `TextEditor` caret |

**Coverage check:** ✅ All current vimcode UI is expressible. The settings panel becomes a `Form`; the explorer becomes a `TreeView`; the source-control panel is a `TreeView` with custom `TreeRow` badges for staged/unstaged state.

### 5.2 SQL client (pgcli / DBeaver-class)

| Screen area | Primitive composition |
|-------------|----------------------|
| Left sidebar | `Panel` with `TreeView` of connections → databases → schemas → tables → columns |
| Main area | `Tabs` — each tab is a query + results. Inside: `Split(vertical) { TextEditor (SQL), DataTable (rows) }` |
| Results view | `DataTable` with sortable columns, cell editing, paging |
| Connection dialog | `Dialog` containing `Form` (host, port, user, password, database) |
| Query history | `Panel` with `ListView` |
| Status bar | `StatusBar` showing connection name, rows returned, query time |
| Command palette | `Palette` for "New connection", "Export results", etc. |

**Gaps found:**
- `DataTable` is a new primitive (not needed by vimcode) with its own complex event surface (sort clicked, cell edited, row selected, column resized). **Must design now**, don't defer — it's essential for any data-oriented app.
- **Column resize** is a new interaction pattern (drag the column separator). Probably a general pattern: divider drags exist in `Split` and `Tabs` too. Worth a shared `Divider` primitive.
- **Multi-select rows with shift-click / ctrl-click** — needs to be a first-class mode in `ListView` and `DataTable`.
- **Export to CSV / long-running tasks** — suggests a `ProgressBar` primitive with cancel, and a toast/notification surface.

### 5.3 k8s dashboard (Lens-class)

| Screen area | Primitive composition |
|-------------|----------------------|
| Left sidebar | `Panel` with `TreeView` of clusters → namespaces → resource kinds (pods, services, deployments…) |
| Main area | Depends on selection: `DataTable` (resource list) OR `Split { DataTable, TextDisplay (YAML/logs) }` |
| Detail pane | `Tabs` with Overview/YAML/Events/Logs/Shell — each is a different primitive |
| Shell tab | `TextEditor` in terminal mode (streaming output, input at bottom) — stretches `TextEditor` |
| Logs tab | `TextDisplay` with live-tail and search |
| Edit YAML | `TextEditor` with YAML syntax highlighting |
| Apply/delete confirms | `Dialog` |
| Cluster switcher | `Palette` |

**Gaps found:**
- **Live-streaming content** — logs and `kubectl exec` output arrive asynchronously. The crate needs a primitive or pattern for "content appended to `TextDisplay`/`TextEditor` from a channel" without the app rebuilding the whole buffer each frame. Probably: `TextDisplay` has an append API the app calls when new data arrives, and only that slice invalidates.
- **Terminal emulation (VT100)** — if an app wants a real shell, the crate needs to either expose a terminal primitive or let the app own one. vimcode already has `terminal_*` methods; consider extracting.
- **Status indicators with colour** — pods have states (Running/Pending/Failed). `ListView`/`DataTable` cells need rich content (coloured dot + text). `StyledText` + per-cell rendering handles this.
- **Custom keyboard shortcuts per-panel** — `l` opens logs, `e` opens edit, etc. The crate must support per-widget keybinding tables, not just global accelerators.

### 5.4 Summary of primitive gaps vs vimcode-only

Building just for vimcode we'd get away with ~12 primitives. To support SQL and k8s apps too, add:

- `DataTable` (with sort, resize, multi-select, cell edit)
- `Divider` (shared drag semantics)
- Live-append support on `TextDisplay` / `TextEditor`
- A terminal primitive (or a documented pattern for hosting one)
- `Toast` + long-running task abstraction with cancel

None of these are surprises. None invalidate the retained-tree + events model. All are achievable in v1 if designed in now.

---

## 6. Backend responsibilities

Each backend implements the same trait surface. Very rough sketch:

```rust
pub trait Backend {
    fn begin_frame(&mut self, viewport: Rect);
    fn end_frame(&mut self) -> Vec<UiEvent>;

    // Primitives
    fn draw_tree(&mut self, tree: &TreeView);
    fn draw_list(&mut self, list: &ListView);
    fn draw_table(&mut self, table: &DataTable);
    fn draw_text_editor(&mut self, ed: &TextEditor);
    fn draw_text_display(&mut self, td: &TextDisplay);
    fn draw_form(&mut self, form: &Form);
    fn draw_tab_bar(&mut self, tb: &TabBar);
    fn draw_status_bar(&mut self, sb: &StatusBar);
    fn draw_activity_bar(&mut self, ab: &ActivityBar);
    fn draw_menu_bar(&mut self, mb: &MenuBar);
    fn draw_palette(&mut self, p: &Palette);
    fn draw_dialog(&mut self, d: &Dialog);
    fn draw_context_menu(&mut self, cm: &ContextMenu);
    fn draw_completions(&mut self, c: &Completions);
    fn draw_tooltip(&mut self, t: &Tooltip);
    fn draw_toast(&mut self, t: &Toast);
    fn draw_spinner(&mut self, s: &Spinner);
    fn draw_progress(&mut self, p: &ProgressBar);

    // Platform services
    fn open_file_dialog(&mut self, opts: FileDialogOptions) -> Option<PathBuf>;
    fn clipboard(&self) -> &dyn Clipboard;
    fn send_notification(&mut self, n: Notification);
}
```

Each primitive's `draw_*` method is responsible for:
- Rendering from the primitive's description
- Producing events from input captured while that primitive was under the pointer / focused
- Retaining primitive-owned state (scroll offset, caret) across frames keyed by `WidgetId`

**What the backend never does:**
- Know about app concepts (no `draw_debug_sidebar`, no `draw_source_control`)
- Retain app state
- Make layout decisions beyond the primitive's own intrinsic sizing
- Hardcode widget colours (always via `PlatformStyle` + `theme.palette`)

---

## 7. Key design decisions requiring a call

Each decision gets a default recommendation in parens; any of them is negotiable.

1. **Retained-tree vs immediate-mode API.** (Retained tree, built each frame. Matches existing `build_screen_layout`.)
2. **Primitive-owned vs app-owned scroll state.** (Primitive-owned keyed by `WidgetId`. Exposed via events for the scroll-binding case.)
3. **One `Backend` trait vs separate traits per primitive.** (One trait, to keep the contract in one file. Primitives with complex state may get helper traits.)
4. **How does a `TextEditor` primitive consume vimcode's Engine?** (The `TextEditor` primitive consumes a `BufferView` description that any app can produce. vimcode's engine becomes an adapter that produces `BufferView` per frame. This cleanly separates vimcode's text model from the generic editor widget.)
5. **Do we support multiple `Window`s per process?** (Yes, v1. Required for detaching tabs, dialogs on multi-monitor, and cross-platform parity. TUI backend collapses to one Window.)
6. **Accessibility.** (v2. Emit an a11y tree but don't wire to platform a11y APIs until there's user demand.)
7. **IME / composition.** (v1.1. Ship without full IME; add when first non-Latin user complains. Text input primitives must at minimum not crash.)
8. **Theming system.** (Palette-based `Theme` struct with derived colours, like vimcode's existing one. Exposed; apps can override. VSCode theme JSON importer.)
9. **Native menu bars.** (v1. macOS uses global menu; Win/Linux uses in-window. Crate owns the platform integration.)
10. **Packaging.** (Single crate `???` with backends behind Cargo features: `win-gui`, `gtk`, `tui`, `cocoa`. Apps pick.)
11. **Naming.** (Working names: `panes`, `uniform`, `uix`, `shipyard-ui`, `cosmos`. Unclaimed on crates.io matters. Decide before extraction.)
12. **Language / toolchain.** (Rust 2021, MSRV tracks latest stable −2. No C/C++ except vendored trees like tree-sitter. mlua for plugin support.)
13. **License.** (MIT + Apache-2.0, standard Rust dual. vimcode's current license prevails.)

---

## 8. Migration path from vimcode

Gradual, in-place. No big-bang rewrite.

**Phase A — primitives live alongside `ScreenLayout` (months 1–2)**
Introduce each primitive's data struct inside `src/render.rs` or a new `src/ui/` module. Implement it in all three existing backends. Replace vimcode's custom panel by panel.

Proposed order (each is ~1–2 sessions of work, small and shippable):

1. `TreeView` — start with source-control panel (simplest, already has `SourceControlData`). Prove the primitive. Replace the GTK `TreeView` widget with `DrawingArea` + `draw_tree`.
2. `TreeView` again — explorer. This is the biggest GTK-specific refactor (native `gtk4::TreeView` → fully-drawn). Does not ship until it matches feature parity with the current explorer.
3. `Form` — replace settings panel. Currently bypasses `ScreenLayout`; this folds it in.
4. `Palette` — command palette already follows the pattern; just formalize the primitive.
5. `ListView` — git status list, quickfix list.
6. `DataTable` — new primitive, no vimcode panel needs it yet. Build while knowledge is fresh; don't use vimcode to validate — build a tiny example app (SQL query runner?) to validate.
7. `StatusBar` / `TabBar` / `ActivityBar` — already mostly unified; finish the migration per issue #133.

**Phase B — extract the crate (month 3)**
Once all panels go through primitives, move `src/ui/` out of `vimcode` into a new workspace member `ui-crate/`. vimcode depends on it as a path dependency. No functional change expected.

**Phase C — macOS backend (month 4, issue #47)**
Implement the `Backend` trait for macOS (Core Graphics + Core Text). Because the trait surface is small and every primitive has a reference implementation in the other three backends, this should be achievable in one focused push. `NATIVE_GUI_LESSONS.md` pre-empts many bugs.

**Phase D — polish + v1 release (month 5)**
Stabilize the API. Write one small example app (SQL runner?) as a second consumer to prove extraction. Document public API. Publish to crates.io.

**Phase E — mature (months 6+)**
Accessibility tree. IME. Animation primitives. Community examples.

This timeline is aggressive but achievable given how much of the work is already done in vimcode. The unknown unknowns are macOS + DataTable + live-streaming content.

---

## 9. Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Primitive set doesn't cover real-world apps and requires frequent breaking changes post-v1 | Medium | Stress-test with SQL and k8s sketches *before* extraction (§5). Build one non-vimcode example app in Phase D. |
| Accessibility debt blocks adoption | Low (for target users) | Document as v2. Target user = keyboard-driven power users who already disable screen readers. |
| Fully-drawn approach feels "off" on macOS vs native Cocoa apps | Medium | Careful `PlatformStyle` work. Use platform font/accent religiously. Hide window chrome behind native title bar. |
| macOS backend blows out timeline due to CG/CT learning curve | Medium-High | Budget extra time. `NATIVE_GUI_LESSONS.md` from Win-GUI should transfer. |
| Text editor primitive too coupled to vimcode's Engine to extract | Medium | `BufferView` adapter pattern (§7.4) separates concerns. Validate during Phase A. |
| Performance regressions from rebuilding description tree every frame on complex UIs | Low | vimcode already does this. Profile before optimising. Add primitive-level dirty bits only if measured. |
| Scope creep — "one more primitive" forever | High | Freeze v1 primitive set after Phase A. Everything else is v1.1+. |

---

## 10. Open questions for you

Before any code is written:

1. **Naming.** Do you have a preference? (See §7.11 for candidates.)
2. **Workspace vs monorepo.** Does the UI crate live inside `vimcode/ui-crate/` as a workspace member, or in a separate repo from day one? (I'd recommend workspace member until v1 to iterate fast, then split.)
3. **Is `TextEditor` part of this crate, or its own crate that this one embeds?** It's huge — syntax, folds, multi-cursor, LSP plumbing. Extracting separately could be cleaner, but then the primitive catalog has a hole. (I lean toward: `TextEditor` is a primitive in this crate; the *text engine* — rope, tree-sitter, LSP — is a separate crate the primitive depends on.)
4. **What's your appetite for accessibility in v1?** Deferring has real users-we-won't-have consequences.
5. **Are there apps you'd want to build personally on this crate after vimcode?** The answer shapes which primitives get priority. (If you've got a SQL client itch, `DataTable` moves up.)
6. **macOS timing.** Block extraction on macOS backend, or extract three-backend first and add macOS after? (I'd recommend extract first, macOS second — don't make the crate release hostage to a whole new backend.)

---

## 11. What happens next

If this sketch is roughly right, the follow-up work is:

1. **Pick 2–3 open questions in §10 and decide.** Especially naming, workspace layout, and whether to build one example app before extraction.
2. **Freeze a v1 primitive list.** Anything not in the list is v1.1.
3. **Write a one-pager for each primitive** — full data struct, full event enum, drawing contract, keyboard contract. These pages become the crate API docs.
4. **Start Phase A.1 — `TreeView` primitive for source-control panel.** Smallest real slice. If it takes less than 2 sessions, the design is sound.

If this sketch is wrong, easiest places it's wrong:
- The retained-tree model is insufficient for `TextEditor` (text editing + IME may need finer-grained incremental updates)
- The primitive set is too coarse (SQL/k8s apps want something not on the list)
- The "fully drawn, no native widgets" call is wrong on macOS specifically (Cocoa conventions run deeper than Win11 and Linux)

Those are the three places to kick the tyres hardest.
