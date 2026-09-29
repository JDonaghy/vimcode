//! Integration tests for the pinned Rust toolchain (#639).
//!
//! Before #639, `.github/workflows/*.yml` installed Rust with
//! `dtolnay/rust-toolchain@stable` and the repo had no `rust-toolchain.toml`,
//! while `ci.yml` gates on `cargo clippy --no-default-features -- -D warnings`.
//! Every six weeks a Rust stable release ships new clippy lints, so **a Rust
//! release could turn CI red with zero code changes** — no commit, no PR,
//! nothing in the history explaining it. That is the same failure class as
//! #615 (ambient `$HOME` leaking into `test_engine`) and #625/#638 (an
//! unpinned quadraui sibling restating snapshots on every machine at once): a
//! build input that moves without a commit.
//!
//! The fix is `rust-toolchain.toml` at the repo root plus an explicit
//! `toolchain:` input on every workflow's install step, so a toolchain bump is
//! a deliberate, reviewable commit. This file is what keeps the two in sync:
//! the workflows cannot be edited back to a floating channel, and they cannot
//! drift to a different version than the one local builds use, without a red
//! test on the branch that does it.
//!
//! Three invariants, each guarding a distinct way the pin can rot:
//!
//! * `rust_toolchain_toml_pins_an_exact_version` — the file exists and names a
//!   concrete `X.Y.Z`, not `stable`/`beta`/`nightly` (a `channel = "stable"`
//!   pin is no pin at all).
//! * `workflows_do_not_use_a_floating_toolchain` +
//!   `workflow_toolchain_matches_rust_toolchain_toml` — no workflow installs a
//!   floating channel, and the version each one does install is the same
//!   version `rust-toolchain.toml` names.
//! * `workflow_toolchain_steps_install_clippy_and_rustfmt` — `dtolnay/rust-toolchain`
//!   installs with `--profile minimal`. That was invisible while the workflows
//!   asked for `stable`, because GitHub runners *preinstall* stable with
//!   clippy and rustfmt already in it. A pinned non-preinstalled version is
//!   downloaded fresh, so `cargo fmt -- --check` / `cargo clippy` would fail
//!   with "no such subcommand" unless the components are requested explicitly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn toolchain_file() -> PathBuf {
    repo_root().join("rust-toolchain.toml")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Every `.github/workflows/*.yml`, keyed by file name.
fn workflows() -> BTreeMap<String, String> {
    let dir = repo_root().join(".github/workflows");
    let mut out = BTreeMap::new();
    for entry in
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("failed to list {}: {e}", dir.display()))
    {
        let path = entry.expect("failed to read a workflow dir entry").path();
        let is_yaml = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "yml" || e == "yaml");
        if !is_yaml {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("workflow file name is not UTF-8")
            .to_string();
        out.insert(name, read(&path));
    }
    assert!(
        !out.is_empty(),
        "no workflow files found under {} — this test would be vacuous",
        dir.display()
    );
    out
}

/// The `channel = "..."` value from `rust-toolchain.toml`.
///
/// Deliberately hand-parsed rather than pulling in a TOML dev-dependency: the
/// file is four lines of `key = "value"` and the point of this test is to have
/// no moving parts of its own.
fn pinned_channel() -> String {
    let path = toolchain_file();
    assert!(
        path.exists(),
        "{} does not exist — without it, `cargo`/`rustup` fall back to \
         whatever toolchain the machine happens to default to, and a Rust \
         stable release can redden `cargo clippy -- -D warnings` with zero \
         code changes (#639).",
        path.display()
    );
    let text = read(&path);
    let channel = toml_string_value(&text, "channel")
        .unwrap_or_else(|| panic!("{} has no `channel = \"...\"` key:\n{text}", path.display()));
    assert!(
        !channel.is_empty(),
        "{} has an empty `channel`",
        path.display()
    );
    channel
}

/// Pull `key = "value"` out of a small TOML file, ignoring `#` comments.
fn toml_string_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        if lhs.trim() != key {
            continue;
        }
        return Some(rhs.trim().trim_matches('"').to_string());
    }
    None
}

/// Pull `key = ["a", "b"]` out of a small TOML file, ignoring `#` comments.
fn toml_array_value(text: &str, key: &str) -> Option<Vec<String>> {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        if lhs.trim() != key {
            continue;
        }
        let inner = rhs.trim().trim_start_matches('[').trim_end_matches(']');
        return Some(
            inner
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        );
    }
    None
}

/// The body of every step in `yaml` that uses `dtolnay/rust-toolchain`.
///
/// Returns `(action_ref, step_text)` pairs — `step_text` is the whole step
/// including its `with:` block, so callers can assert on the inputs. Note the
/// `uses:` key is *not* assumed to come first: these workflows write
/// `- name: Install Rust toolchain` and then `uses:` on the next line.
fn rust_toolchain_steps(yaml: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- ") {
            i += 1;
            continue;
        }
        let indent = line.len() - trimmed.len();
        // Collect the whole list item: everything up to the next line that is
        // either a new item at this indent or dedents out of the list.
        let mut body = String::from(line);
        let mut j = i + 1;
        while j < lines.len() {
            let next = lines[j];
            if next.trim().is_empty() {
                body.push('\n');
                j += 1;
                continue;
            }
            let next_indent = next.len() - next.trim_start().len();
            if next_indent < indent
                || (next_indent == indent && next.trim_start().starts_with("- "))
            {
                break;
            }
            body.push('\n');
            body.push_str(next);
            j += 1;
        }
        if let Some(action_ref) = body.lines().find_map(|l| {
            l.trim()
                .trim_start_matches("- ")
                .strip_prefix("uses:")
                .map(str::trim)
                .and_then(|u| u.strip_prefix("dtolnay/rust-toolchain@"))
        }) {
            out.push((action_ref.trim().to_string(), body));
        }
        i = j;
    }
    out
}

#[test]
fn step_parser_finds_a_toolchain_step_whose_uses_is_not_the_first_key() {
    // Guards this file's own parser: an earlier version only matched
    // `- uses: dtolnay/...` and silently found zero steps in workflows that
    // write `- name:` first — which made the component assertions vacuously
    // green. The `checked > 0` guard in
    // `workflow_toolchain_matches_rust_toolchain_toml` is the other half.
    let yaml = "\
jobs:
  test:
    steps:
    - uses: actions/checkout@v4

    - name: Install Rust toolchain
      uses: dtolnay/rust-toolchain@master
      with:
        toolchain: ${{ env.RUST_TOOLCHAIN }}
        components: clippy, rustfmt

    - name: Build
      run: cargo build
";
    let steps = rust_toolchain_steps(yaml);
    assert_eq!(steps.len(), 1, "expected exactly one toolchain step");
    assert_eq!(steps[0].0, "master");
    assert!(steps[0].1.contains("components: clippy, rustfmt"));
    // The following unrelated step must not be swallowed into the body.
    assert!(!steps[0].1.contains("cargo build"));
}

#[test]
fn rust_toolchain_toml_pins_an_exact_version() {
    let channel = pinned_channel();
    let exact = channel.split('.').count() == 3
        && channel
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    assert!(
        exact,
        "rust-toolchain.toml pins `channel = \"{channel}\"`, which is not an \
         exact `X.Y.Z` version. A floating channel (`stable`, `beta`, \
         `nightly`, or a two-component `1.97`) still lets a Rust release move \
         the compiler under CI with no commit — the whole point of #639."
    );
}

#[test]
fn rust_toolchain_toml_requests_clippy_and_rustfmt() {
    let text = read(&toolchain_file());
    let components = toml_array_value(&text, "components").unwrap_or_default();
    for needed in ["clippy", "rustfmt"] {
        assert!(
            components.iter().any(|c| c == needed),
            "rust-toolchain.toml does not list `{needed}` in `components` \
             (found {components:?}). `rustup` installs a toolchain-file pin \
             with exactly the components named there, so `cargo {needed}` \
             would not exist on a fresh checkout."
        );
    }
}

#[test]
fn workflows_do_not_use_a_floating_toolchain() {
    for (name, yaml) in workflows() {
        for floating in ["@stable", "@beta", "@nightly"] {
            let needle = format!("dtolnay/rust-toolchain{floating}");
            assert!(
                !yaml.contains(&needle),
                ".github/workflows/{name} installs Rust with \
                 `{needle}` — a floating channel. Every Rust release ships \
                 new clippy lints, and ci.yml gates on `-D warnings`, so this \
                 lets a release redden CI with zero code changes (#639). Use \
                 `dtolnay/rust-toolchain@master` with an explicit \
                 `toolchain:` input instead."
            );
        }
    }
}

#[test]
fn workflow_toolchain_matches_rust_toolchain_toml() {
    let channel = pinned_channel();
    let mut checked = 0usize;
    for (name, yaml) in workflows() {
        let steps = rust_toolchain_steps(&yaml);
        if steps.is_empty() {
            continue;
        }
        // Each workflow declares the pin once, at workflow level, so the
        // per-step inputs cannot disagree with each other.
        let env_line = yaml
            .lines()
            .find_map(|l| l.trim().strip_prefix("RUST_TOOLCHAIN:").map(str::trim))
            .map(|v| v.trim_matches('"').to_string());
        assert_eq!(
            env_line.as_deref(),
            Some(channel.as_str()),
            ".github/workflows/{name} installs Rust but its workflow-level \
             `env.RUST_TOOLCHAIN` is {env_line:?}, not the \
             `{channel}` that rust-toolchain.toml pins. CI would then build \
             with a different compiler than every local `cargo build` (#639)."
        );

        for (action_ref, step) in steps {
            checked += 1;
            assert_eq!(
                action_ref, "master",
                ".github/workflows/{name} uses \
                 `dtolnay/rust-toolchain@{action_ref}`; use `@master` with an \
                 explicit `toolchain:` input so the Rust version is pinned by \
                 this repo rather than by the action's branch (#639)."
            );
            let toolchain_input = step
                .lines()
                .find_map(|l| l.trim().strip_prefix("toolchain:").map(str::trim))
                .map(|v| v.trim_matches('"').to_string());
            let accepted = ["${{ env.RUST_TOOLCHAIN }}".to_string(), channel.clone()];
            assert!(
                toolchain_input
                    .as_deref()
                    .is_some_and(|t| accepted.iter().any(|a| a == t)),
                ".github/workflows/{name} has a `dtolnay/rust-toolchain` step \
                 whose `toolchain:` input is {toolchain_input:?}; expected one \
                 of {accepted:?} so it tracks rust-toolchain.toml (#639).\n\
                 Step was:\n{step}"
            );
        }
    }
    assert!(
        checked > 0,
        "no `dtolnay/rust-toolchain` steps found in any workflow — this test \
         would be vacuously green"
    );
}

#[test]
fn workflow_toolchain_steps_install_clippy_and_rustfmt() {
    for (name, yaml) in workflows() {
        for (_, step) in rust_toolchain_steps(&yaml) {
            let components = step
                .lines()
                .find_map(|l| l.trim().strip_prefix("components:").map(str::trim))
                .unwrap_or("")
                .to_string();
            for needed in ["clippy", "rustfmt"] {
                assert!(
                    components.contains(needed),
                    ".github/workflows/{name} has a `dtolnay/rust-toolchain` \
                     step that does not request `{needed}` in `components:` \
                     (found {components:?}). The action installs with \
                     `--profile minimal`; that went unnoticed while the \
                     workflows asked for `stable`, because GitHub runners \
                     preinstall stable *with* clippy and rustfmt. A pinned \
                     version is downloaded fresh, so `cargo {needed}` would \
                     not exist (#639).\nStep was:\n{step}"
                );
            }
        }
    }
}
