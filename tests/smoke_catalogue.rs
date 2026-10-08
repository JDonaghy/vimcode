//! Schema test for `tests/smoke-spec/catalogue.yaml` (#1729).
//!
//! `claude-coordinator`'s `coord bugbash vimcode` (claude-coordinator#3580)
//! reads that file to drive its bug-bash journeys. This test is the guard
//! that keeps the catalogue honest: a malformed entry (duplicate id, an enum
//! value outside the schema's known set, an empty `expected`) must fail CI
//! *here*, in this repo, rather than silently degrading bugbash's coverage
//! at run time in a different repo.
//!
//! RED-verified (#1729): temporarily duplicating an `id` and blanking an
//! `expected` field each independently failed `catalogue_schema_is_valid`
//! with a clear panic message before the catalogue content below was
//! restored.

use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Catalogue {
    version: u32,
    journeys: Vec<Journey>,
}

#[derive(Debug, Deserialize)]
struct Journey {
    id: String,
    area: String,
    mode: String,
    lanes: Vec<String>,
    reference: String,
    reference_detail: String,
    steps: String,
    expected: String,
    priority: u32,
}

const KNOWN_AREAS: &[&str] = &[
    "vim-mode",
    "vscode-mode",
    "mode-switch",
    "chrome",
    "platform-input",
    "render",
    "terminal",
    "idle",
];

const KNOWN_MODES: &[&str] = &["vim", "vscode", "any"];

const KNOWN_REFERENCES: &[&str] = &["nvim", "vscode", "platform", "spec"];

const KNOWN_LANES: &[&str] = &["tui-pty", "win-native", "mac-native", "gtk-native"];

const KNOWN_PRIORITIES: &[u32] = &[1, 2, 3];

fn catalogue_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/smoke-spec/catalogue.yaml")
}

fn load_catalogue() -> Catalogue {
    let text = std::fs::read_to_string(catalogue_path())
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", catalogue_path().display()));
    serde_yaml::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", catalogue_path().display()))
}

/// The real acceptance test: the shipped catalogue parses and every journey
/// in it satisfies the schema's constraints.
#[test]
fn catalogue_schema_is_valid() {
    let cat = load_catalogue();
    validate(&cat);
}

/// Separate from the file-driven test above so the *targets* (minimum
/// journey counts per area, from #1729's issue body) are pinned as their own
/// named, independently-failing assertions rather than folded into one
/// generic validation pass.
#[test]
fn catalogue_meets_coverage_targets() {
    let cat = load_catalogue();

    let count = |area: &str| cat.journeys.iter().filter(|j| j.area == area).count();
    let vscode_mode = count("vscode-mode");
    let vim_mode = count("vim-mode");
    let mode_switch = count("mode-switch");
    let chrome = count("chrome");
    let platform_and_render = cat
        .journeys
        .iter()
        .filter(|j| {
            matches!(
                j.area.as_str(),
                "platform-input" | "render" | "terminal" | "idle"
            )
        })
        .count();

    assert!(
        vscode_mode >= 60,
        "vscode-mode journeys: {vscode_mode} (need >= 60)"
    );
    assert!(vim_mode >= 40, "vim-mode journeys: {vim_mode} (need >= 40)");
    assert!(
        mode_switch >= 5,
        "mode-switch journeys: {mode_switch} (need >= 5)"
    );
    assert!(
        platform_and_render >= 25,
        "platform-input/render/terminal/idle journeys: {platform_and_render} (need >= 25)"
    );
    assert!(chrome >= 15, "chrome journeys: {chrome} (need >= 15)");

    let total = cat.journeys.len();
    let priority1 = cat.journeys.iter().filter(|j| j.priority == 1).count();
    // "Keep priority 1 to roughly a third of the total" -- a loose band, not
    // an exact fraction: catches a catalogue that drifts to "everything is
    // priority 1" (which makes the priority field meaningless) or "nothing
    // is" (which makes it useless for release triage), without demanding an
    // exact 33.33%.
    let lower = total / 5; // 20%
    let upper = total / 2; // 50%
    assert!(
        priority1 >= lower && priority1 <= upper,
        "priority-1 journeys: {priority1} of {total} total (expected roughly a third, \
         i.e. between {lower} and {upper})"
    );
}

fn validate(cat: &Catalogue) {
    assert_eq!(cat.version, 1, "unexpected catalogue schema version");
    assert!(!cat.journeys.is_empty(), "catalogue has zero journeys");

    let mut seen_ids: HashSet<&str> = HashSet::new();

    for j in &cat.journeys {
        assert!(!j.id.trim().is_empty(), "journey with a blank id");
        assert!(
            j.id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "id {:?} is not kebab-case (lowercase ascii, digits, '-' only)",
            j.id
        );
        assert!(
            seen_ids.insert(j.id.as_str()),
            "duplicate journey id: {:?}",
            j.id
        );

        assert!(
            KNOWN_AREAS.contains(&j.area.as_str()),
            "journey {:?}: unknown area {:?} (known: {KNOWN_AREAS:?})",
            j.id,
            j.area
        );
        assert!(
            KNOWN_MODES.contains(&j.mode.as_str()),
            "journey {:?}: unknown mode {:?} (known: {KNOWN_MODES:?})",
            j.id,
            j.mode
        );
        assert!(
            KNOWN_REFERENCES.contains(&j.reference.as_str()),
            "journey {:?}: unknown reference {:?} (known: {KNOWN_REFERENCES:?})",
            j.id,
            j.reference
        );
        assert!(
            KNOWN_PRIORITIES.contains(&j.priority),
            "journey {:?}: priority {} outside {KNOWN_PRIORITIES:?}",
            j.id,
            j.priority
        );

        assert!(
            !j.lanes.is_empty(),
            "journey {:?}: lanes must not be empty",
            j.id
        );
        for lane in &j.lanes {
            assert!(
                KNOWN_LANES.contains(&lane.as_str()),
                "journey {:?}: unknown lane {:?} (known: {KNOWN_LANES:?})",
                j.id,
                lane
            );
        }

        assert!(
            !j.reference_detail.trim().is_empty(),
            "journey {:?}: reference_detail must not be empty",
            j.id
        );
        assert!(
            !j.steps.trim().is_empty(),
            "journey {:?}: steps must not be empty",
            j.id
        );
        assert!(
            !j.expected.trim().is_empty(),
            "journey {:?}: expected must not be empty",
            j.id
        );
    }
}

/// #1729's extensions-out-of-scope rule: no journey should mention installing
/// or invoking the extension system (`:ExtInstall`, LSP/DAP install flows are
/// a different, already-catalogued-elsewhere concern from "install an
/// extension").
#[test]
fn catalogue_has_no_extension_install_journeys() {
    let cat = load_catalogue();
    for j in &cat.journeys {
        let haystack = format!("{} {} {}", j.steps, j.expected, j.reference_detail).to_lowercase();
        assert!(
            !haystack.contains("extinstall") && !haystack.contains("install an extension"),
            "journey {:?} looks like an extension-install journey, out of scope per #1729",
            j.id
        );
    }
}

// ---------------------------------------------------------------------------
// RED-verification harness: these two tests prove `catalogue_schema_is_valid`
// actually fails on a malformed catalogue, rather than trivially passing on
// anything. They parse a deliberately-broken inline fixture, never the real
// file, so they stay green permanently (they are the regression guard for
// the *validator*, not for the catalogue content).
// ---------------------------------------------------------------------------

fn parse_str(yaml: &str) -> Result<Catalogue, serde_yaml::Error> {
    serde_yaml::from_str(yaml)
}

fn minimal_valid_journey(id: &str) -> String {
    format!(
        "- id: {id}\n  area: vim-mode\n  mode: vim\n  lanes: [tui-pty]\n  reference: nvim\n  \
         reference_detail: test fixture\n  steps: press x\n  expected: x happens\n  priority: 1\n"
    )
}

#[test]
fn duplicate_id_fails_validation() {
    let yaml = format!(
        "version: 1\njourneys:\n{}{}",
        minimal_valid_journey("dup-journey"),
        minimal_valid_journey("dup-journey")
    );
    let cat = parse_str(&yaml).expect("fixture itself must parse as YAML");
    let result = std::panic::catch_unwind(|| validate(&cat));
    assert!(
        result.is_err(),
        "validate() must reject a catalogue with a duplicate id"
    );
}

#[test]
fn empty_expected_fails_validation() {
    let yaml = "version: 1\njourneys:\n- id: broken-journey\n  area: vim-mode\n  mode: vim\n  \
                lanes: [tui-pty]\n  reference: nvim\n  reference_detail: test fixture\n  \
                steps: press x\n  expected: ''\n  priority: 1\n";
    let cat = parse_str(yaml).expect("fixture itself must parse as YAML");
    let result = std::panic::catch_unwind(|| validate(&cat));
    assert!(
        result.is_err(),
        "validate() must reject a journey with an empty expected field"
    );
}

#[test]
fn unknown_area_fails_validation() {
    let yaml =
        "version: 1\njourneys:\n- id: broken-journey\n  area: not-a-real-area\n  mode: vim\n  \
                lanes: [tui-pty]\n  reference: nvim\n  reference_detail: test fixture\n  \
                steps: press x\n  expected: x happens\n  priority: 1\n";
    let cat = parse_str(yaml).expect("fixture itself must parse as YAML");
    let result = std::panic::catch_unwind(|| validate(&cat));
    assert!(
        result.is_err(),
        "validate() must reject a journey with an unknown area"
    );
}

#[test]
fn unknown_lane_fails_validation() {
    let yaml = "version: 1\njourneys:\n- id: broken-journey\n  area: vim-mode\n  mode: vim\n  \
                lanes: [not-a-real-lane]\n  reference: nvim\n  reference_detail: test fixture\n  \
                steps: press x\n  expected: x happens\n  priority: 1\n";
    let cat = parse_str(yaml).expect("fixture itself must parse as YAML");
    let result = std::panic::catch_unwind(|| validate(&cat));
    assert!(
        result.is_err(),
        "validate() must reject a journey with an unknown lane"
    );
}
