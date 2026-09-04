# Common Patterns

Load this file when adding new features, keys, commands, settings, or theme colors.

## Adding Features

**Add Normal Mode Key:** `engine/keys.rs` → `handle_normal_key()` → add match arm → test

**Add Command:** `engine/execute.rs` → `execute_command()` → add match arm → test

**Add Operator+Motion:** `engine/keys.rs` → set `pending_operator` → implement in `handle_operator_motion()` → test

**Ctrl-W Command:** `engine/keys.rs` → `handle_pending_key()` under `'\x17'` case

**Engine Facade Methods:** `buffer()`, `buffer_mut()`, `view()`, `view_mut()`, `cursor()` — all operate on active window's buffer

**Show User-Facing Info (About, errors, confirmations):** Use the modal dialog system (`show_dialog()` / `show_error_dialog()`) rather than `self.message`. Dialogs are preferred for anything that deserves user attention — the message bar is for transient status only.

## Hit Regions for Clickable UI

When adding clickable elements to the find/replace overlay (or future UI panels), define hit regions in `engine/mod.rs` using `FrHitRegion` + `FindReplaceClickTarget` types. Compute regions once in `build_screen_layout()`, then backends walk the region list to resolve clicks. Dispatch through a shared `Engine::handle_*_click()` method. This avoids per-backend geometry duplication. See `compute_find_replace_hit_regions()` as the reference implementation.

## Adding a New Setting

Update ALL of these:
1. Add field to `Settings` struct in `settings.rs` with `#[serde(default = "default_fn_name")]`
2. Create default function returning sensible default value
3. Update `Default` impl to include the field
4. Add to `get_value_str()` and `set_value_str()` in `settings.rs`
5. Add a `SettingDef` entry to `SETTING_DEFS` in `render.rs` (controls the Settings sidebar UI)
6. Settings are automatically merged: new fields are added to existing settings files without overwriting user values
7. Document the setting name and purpose in comments

## Hermetic Engine Construction in Tests

**Any new hermetic/rendering/snapshot-style test that needs an `Engine` should call `Engine::new_for_test()`, not `Engine::new()`.** `new_for_test()` builds settings/session/history/`git_branch` from in-memory defaults instead of loading ambient disk/git state, so tests stay reproducible regardless of the machine, `$HOME`, or git branch they run on (#439, #615, #617). Don't call `Engine::new()` and overwrite fields afterward — some ambient state (e.g. `session.explorer_visible`) is consumed inside construction to drive `app_shell.hide_sidebar()` before a post-hoc field reset can run, so overwrite-after doesn't reliably undo it. See `src/tui_main/render_impl.rs`'s `test_engine()` for the reference pattern, including how to reset `extension_state`/`ext_registry` (still loaded from disk/cache unconditionally) and how to flip sidebar visibility through `app_shell`'s own API rather than reassigning `session` directly.

## Theme Colors (CRITICAL)

**NEVER introduce new hex color literals for derived theme fields.** Every new color added to the `Theme` struct must be derived from an existing foundational theme field (`background`, `foreground`, etc.) using `lighten()`/`darken()`/`cursorline_tint()`/`colorcolumn_tint()` or similar. Use a local variable to avoid repeating hex strings:
```rust
pub fn onedark() -> Self {
    let bg = Color::from_hex("#1a1a1a");
    Self {
        background: bg,
        new_derived_color: bg.some_tint(),  // GOOD: derived from variable
        // bad_color: Color::from_hex("#2c313a"),  // BAD: hardcoded hex
    }
}
```
**Why:** Hardcoded hex values don't adapt to custom themes or VSCode theme imports. Only foundational colors (background, foreground, keyword, string, etc.) should have hex literals.

## Vim-behaviour Changes: the Neovim Oracle Is the Acceptance Bar (#799)

**Any PR that changes how VimCode responds to Vim keystrokes must add cases to
`tests/nvim_conformance.rs`.** Hand-written expectations are second-class: a test
that asserts `assert_eq!(buf, "expected")` encodes *the author's belief* about
what Vim does, and a shared misconception passes such a test forever. The repo
has ~1,300 such tests and a live example of the failure mode —
`tests/operator_motions.rs::test_dj_at_last_line_noop_or_delete_last`, whose
comment states the wrong Vim behaviour and whose assertion is too weak to catch
either answer.

`tests/nvim_conformance.rs` is the only suite where nothing is hand-authored: the
same keystrokes run through `nvim --headless` and through `Engine`, and buffer +
cursor are compared. It carries 1,432 cases across 16 areas. Add yours to the
relevant `CASES_*` array with `c(label, lines, line, col, keys)`, or `cs(..)` when
you need a line of Lua `setup` to pin a Vim-vs-Neovim option default
(`startofline`, `joinspaces`, `nrformats`, `smarttab`, …) — without that you
cannot tell "VimCode differs from Vim" apart from "Neovim differs from Vim".

`KNOWN_DEVIATIONS` in that file lists the labels that currently differ. The gate
is **bidirectional and the list may only ever shrink**:

* an unlisted label that fails → regression, the test fails;
* a listed label that starts passing → the fix must delete its entry, or the test
  fails.

So a Vim-compat fix proves itself by *deleting lines from `KNOWN_DEVIATIONS`*,
not by adding an assertion. Regenerate the list only after a deliberate change:

```sh
CONFORMANCE_DUMP_DEVIATIONS=/tmp/dev.txt \
  cargo test --no-default-features --test nvim_conformance -- --nocapture
PROBE_FILTER=<label-substring> cargo test --test nvim_conformance -- --nocapture
```

Never regenerate to paper over a regression — the list not growing is the whole
point. Requires `nvim` on PATH; CI installs it (#795) and hard-fails if it's
missing, so a skip locally is not a green light.
