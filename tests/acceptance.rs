//! Sealed acceptance-suite crate root (#657) — the oracle-loop entrypoint.
//!
//! # Why this file exists at all
//!
//! Every black-box test in this repo used to live in-crate, under
//! `#[cfg(test)]`, because it *had to*: `tests/*.rs` is a separate crate that
//! links only against `[lib] vimcode_core`, and until #657 that library
//! exposed nothing but `core`, `icons` and `quadraui_pin`. The GTK backend,
//! the TUI backend, `render`, and the #646 `GtkDriver` harness all lived
//! inside the `vimcode` *binary*, invisible from here.
//!
//! That made the coordinator's sealed-suite oracle loop impossible for
//! vimcode: `coord/acceptance.py` hardcodes `ACCEPTANCE_DIRNAME =
//! "tests/acceptance"`, so an in-crate `#[cfg(test)] mod acceptance` is not
//! an option — the suite must be a real integration-test target. #657 Stage 1
//! promoted `render`, `tui_main` and `gtk` into `vimcode_core` to unblock it;
//! this file is Stage 2.
//!
//! # What "sealed" means
//!
//! Slices under `tests/acceptance/ms-NN/` are authored by the `test-author`
//! agent from a Gate-A contract, with **zero** implementation context, and
//! the worker fixing the issue may *run* them (`coord acceptance run --issue
//! N`) but may not read or edit them. That independence is the whole point:
//! #553 in this repo shipped self-authored black-box tests that stayed green
//! with the bug reinstated, and only adversarial review caught it. A tamper
//! gate in `coord/merge_queue.py` enforces the seal on the
//! `tests/acceptance/` path.
//!
//! # Running it
//!
//! ```text
//! cargo test --test acceptance --features test-support
//! ```
//!
//! The configured driver command (`kind: tui-tuidriver`, which is really the
//! generic stdout-native driver — see `_run_generic` in
//! `coord/acceptance_drivers.py`) adds libtest JSON output:
//!
//! ```text
//! RUSTC_BOOTSTRAP=1 cargo test --test acceptance --features test-support \
//!   -- -Z unstable-options --format json
//! ```
//!
//! `RUSTC_BOOTSTRAP=1` is what makes `--format json` accepted by a *stable*
//! rustc; without it the run fails outright with "only accepted on the
//! nightly compiler".
//!
//! `Cargo.toml`'s `[[test]] name = "acceptance"` stanza pins
//! `required-features = ["test-support", "gui"]`, so a driver invocation that
//! forgets the flags fails loudly instead of compiling to zero tests and
//! reporting a vacuous "0 passed".
#![cfg(all(feature = "test-support", feature = "gui"))]

// ── Sealed oracle suite (docs/ORACLE_LOOP.md) — DO NOT REMOVE ─────────────
// Each milestone's independently-authored slice is `include!`d here at crate
// root so the `--test acceptance` target runs it. The slice files hold the
// assertions; this file only pastes them in. Slice directories are named
// `ms-NN` (hyphens — not valid Rust module names), which is why these are
// `include!`s rather than `mod` declarations. Each slice wraps its tests in
// its own `mod`, so the libtest ids the manifest maps are module-qualified.
include!("acceptance/ms-example/seam_657.rs");
