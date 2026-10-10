#!/usr/bin/env python3
"""Validate `tests/smoke-spec/*.yaml` (vimcode#1646, vimcode#1884).

Two independent kinds of checking happen here:

1. **Parse-against-the-real-driver check** (`check_coord_parsing`,
   vimcode#1646): each Tier-2 spec file is consumed by one of coord's real
   acceptance drivers (`coord/tui_pty_driver.py`, `coord/win_native_driver.py`,
   `coord/mac_native_driver.py`, `coord/gtk_native_driver.py`,
   claude-coordinator `main`, v0.5.551+). This imports each driver's own
   `parse_smoke_spec`/`parse_native_spec` and runs it against the matching
   file, so "does this spec parse" is answered by the exact code that will
   actually run it, not a hand-rolled shape check that could drift from what
   the real driver accepts. Requires the `coord` package importable
   (`pip install code-coordinator`).

2. **Static structural checks** (vimcode#1884): a cost-analysis (2026-10-09)
   of vimcode#1831/#1833/#1834/#1835's review rounds found the same three
   step-authoring mistakes burning a review round each, every time:
   - a `known_bug`-tagged step that cannot actually fail (so it *always*
     "passes", which coord's `classify_step` reads as "bug fixed, closing");
   - a stale `expect_file` marker reused across steps/runs with no cleanup,
     so a leftover artifact from an earlier run masks a real failure;
   - a raw control character (`\r`/`\n`/`\x1b`) embedded in `type_text` on a
     driver that delivers it one code-unit at a time via OS input injection,
     which silently drops it instead of submitting anything.
   These are all checkable by reading the YAML directly — no `coord` import
   needed — so `main()` below runs the real-driver check if `coord` is
   installed AND always runs the static checks regardless, since CI and a
   local dev loop without `code-coordinator` installed both want the latter.

Usage:
    python3 scripts/validate_smoke_specs.py
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import sys

import yaml

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
SPEC_DIR = REPO_ROOT / "tests" / "smoke-spec"

# The six driver-specific Tier-2 spec files, each pinned to exact steps for a
# handful of previously-reported bugs and marked additive-only (vimcode#3509):
# a later PR may only APPEND new steps to one of these, never edit or remove
# an existing one -- a correction to a stale step is made by adding a new,
# freshly-authored step alongside it (see `tui.yaml`'s own `-1740` section
# for the established precedent), not by rewriting history. `catalogue.yaml`
# is deliberately excluded: it is a different schema (one-line journeys, no
# `steps:`/`type_text`/`known_bug`), checked by `tests/smoke_catalogue.rs`
# instead.
SEALED_SPEC_FILES = [
    "tui.yaml",
    "gtk-gui.yaml",
    "mac-gui.yaml",
    "mac-gtk.yaml",
    "win-gui.yaml",
    "win-terminal.yaml",
]

# `tui.yaml` (tui-pty) and `win-terminal.yaml` (win-native, `mode: terminal`)
# send `type_text` straight into a real pty/ConPTY raw byte stream, where a
# `\r`/`\x1b` is a legitimate, necessary byte (submitting an ex command,
# leaving a mode) -- see both files' own `review round 1` headers. Every
# other spec types text via OS input injection (`SendInput`/`CGEvent`-style),
# which drops control characters silently (`events::wm_char_to_uievent`,
# quadraui 0.1.2) -- those files must submit/escape via a separate
# `- type: key` step instead.
RAW_STREAM_LANES = {"tui.yaml", "win-terminal.yaml"}

CONTROL_CHAR_PATTERN = re.compile(r"[\r\n\x1b]")

# Step types that cannot fail, or whose assertion is structurally a
# NEGATIVE one (passes in nearly every state): tagging either with
# `known_bug` guarantees a PASS while the referenced bug is open, which
# `coord.nightly_smoke.classify_step` reads as `GREEN_KNOWN_BUG_FIXED` and
# acts on by closing the referenced issue (vimcode#1831 review round 2).
#   - `wait` always succeeds; `launch` always succeeds (if it didn't, every
#     later step would fail too, not just this one).
#   - `expect_no_tofu` is negative: `looks_like_tofu` returns `(False, ...)`
#     — PASS — unless it finds a near-uniform interior AND a high-contrast
#     border; a blank/empty region is explicitly not tofu, so it PASSES over
#     exactly the kind of "nothing rendered yet" state a bug often produces.
CANNOT_FAIL_STEP_TYPES = {"wait", "launch", "expect_no_tofu"}

# Pre-existing violations found by this validator that predate it and are
# intentionally left un-edited, per the additive-only policy (vimcode#3509):
# the fix for a stale/wrong existing step is a NEW corrective step placed
# alongside it (see `tui.yaml`'s own `-1740` section), not a rewrite of the
# existing one -- and deciding whether to outright untag vs. replace this
# one is a human call this validator should not make silently. Printed as
# KNOWN (not FAIL) when matched; removing an entry here without the real fix
# landing is exactly what CLAUDE.md "Testing (CRITICAL)" rule 3's
# bidirectional-gate requirement exists to prevent, so each entry names the
# vimcode issue that tracks the actual fix.
#
# Keyed by (filename, rule_name, step_id) -> human-readable reference.
KNOWN_VIOLATIONS: dict[tuple[str, str, str], str] = {
    ("win-gui.yaml", "known_bug_inversion", "activity-bar-explorer-icon-no-tofu-178"): (
        "vimcode#178 -- expect_no_tofu is a negative assertion that passes "
        "over a blank/not-yet-rendered region, so pass+known_bug risks "
        "coord's classify_step auto-closing #178 on the first real Win-GUI "
        "run. Found by this validator (vimcode#1884); left unedited per "
        "the additive-only policy (vimcode#3509) pending a human call on "
        "untagging vs. replacing it with a freshly-authored region check."
    ),
}


def _steps_of(data: object) -> list[dict]:
    if not isinstance(data, dict):
        return []
    steps = data.get("steps")
    if not isinstance(steps, list):
        return []
    return [s for s in steps if isinstance(s, dict)]


def expect_file_marker_violations(steps: list[dict]) -> list[dict]:
    """Rule: every `expect_file` step's `contains` marker must be unique
    within the file.

    vimcode#1831/#1835 review rounds independently found the same bug class:
    an `expect_file` probe asserting on a fixed marker/path with no
    per-run uniqueness and no cleanup step, so a leftover file from an
    earlier run satisfies a LATER run's check even if that run's own
    command never actually executed -- a permanent stale-pass. Two
    different steps asserting the identical literal marker is the sharpest,
    statically-checkable symptom of that: either step's artifact can
    silently satisfy the other.
    """
    seen: dict[object, str] = {}
    violations: list[dict] = []
    for step in steps:
        if step.get("type") != "expect_file":
            continue
        marker = step.get("contains")
        if marker is None:
            continue
        step_id = step.get("id", "<no id>")
        if marker in seen:
            violations.append(
                {
                    "rule": "expect_file_marker_uniqueness",
                    "step_id": step_id,
                    "message": (
                        f"expect_file step {step_id!r} reuses contains "
                        f"marker {marker!r}, already used by step "
                        f"{seen[marker]!r} -- a stale file left by either "
                        "step's earlier run could satisfy the other, "
                        "masking a real failure (vimcode#1831/#1835)"
                    ),
                }
            )
        else:
            seen[marker] = step_id
    return violations


def known_bug_inversion_violations(steps: list[dict]) -> list[dict]:
    """Rule: `known_bug` may not tag a step type that cannot fail."""
    violations: list[dict] = []
    for step in steps:
        if "known_bug" not in step:
            continue
        step_type = step.get("type")
        if step_type in CANNOT_FAIL_STEP_TYPES:
            step_id = step.get("id", "<no id>")
            violations.append(
                {
                    "rule": "known_bug_inversion",
                    "step_id": step_id,
                    "message": (
                        f"step {step_id!r} (type: {step_type}) is tagged "
                        f"known_bug: {step['known_bug']!r}, but {step_type!r} "
                        "steps cannot fail -- an inevitable PASS with a "
                        "known_bug tag makes coord's classify_step report "
                        "GREEN_KNOWN_BUG_FIXED and auto-close the referenced "
                        "issue while it is still broken (vimcode#1831 "
                        "review round 2)"
                    ),
                }
            )
    return violations


def control_char_violations(steps: list[dict], filename: str) -> list[dict]:
    """Rule: `type_text` must not embed a raw `\\r`/`\\n`/`\\x1b` on a
    native-GUI lane, where it is silently dropped instead of submitted.
    """
    if filename in RAW_STREAM_LANES:
        return []
    violations: list[dict] = []
    for step in steps:
        if step.get("type") != "type_text":
            continue
        text = step.get("text")
        if not isinstance(text, str):
            continue
        found = sorted(set(CONTROL_CHAR_PATTERN.findall(text)))
        if not found:
            continue
        step_id = step.get("id", "<no id>")
        escaped = ", ".join(repr(c) for c in found)
        violations.append(
            {
                "rule": "type_text_control_char",
                "step_id": step_id,
                "message": (
                    f"type_text step {step_id!r} embeds raw control "
                    f"character(s) {escaped} in its text -- this lane types "
                    "text one character/code-unit at a time via OS input "
                    "injection, which drops control characters silently "
                    "instead of submitting anything; use a separate "
                    "`- type: key` step (key: enter / key: esc) instead "
                    "(vimcode#1833/#1834 review round 1)"
                ),
            }
        )
    return violations


def additive_only_violations(old_steps: list[dict], new_steps: list[dict]) -> list[dict]:
    """Rule: a sealed spec's existing steps (by `id`) must survive unchanged.

    Compares two already-parsed step lists -- callers are responsible for
    sourcing `old_steps` from whatever "sealed" baseline matters (typically
    the file's content at the PR's base ref) and `new_steps` from the
    working tree. A step `id` present in `old_steps` but missing, or present
    with different content, in `new_steps` is a violation; a step `id` only
    present in `new_steps` (a genuinely new, appended step) is not.
    """
    old_by_id = {s["id"]: s for s in old_steps if "id" in s}
    new_by_id = {s["id"]: s for s in new_steps if "id" in s}
    violations: list[dict] = []
    for step_id, old_step in old_by_id.items():
        if step_id not in new_by_id:
            violations.append(
                {
                    "rule": "additive_only",
                    "step_id": step_id,
                    "message": (
                        f"step {step_id!r} was removed -- this spec is "
                        "additive-only (vimcode#3509); a correction to an "
                        "existing step is made by adding a new step "
                        "alongside it, not by deleting the old one"
                    ),
                }
            )
        elif new_by_id[step_id] != old_step:
            violations.append(
                {
                    "rule": "additive_only",
                    "step_id": step_id,
                    "message": (
                        f"step {step_id!r} was modified -- this spec is "
                        "additive-only (vimcode#3509); append a new step "
                        "instead of editing an existing one"
                    ),
                }
            )
    return violations


def _resolve_base_commit() -> str | None:
    """Best-effort merge-base with the PR's base branch, for the
    additive-only check. Returns `None` (never raises) if it cannot be
    resolved -- e.g. a shallow checkout that never fetched the base branch
    -- in which case the caller skips rule 4 for this run rather than
    failing on infrastructure it doesn't control.
    """
    override = os.environ.get("SMOKE_SPEC_BASE_REF")
    candidates = [override] if override else ["origin/develop", "develop"]
    for ref in candidates:
        if not ref:
            continue
        try:
            result = subprocess.run(
                ["git", "merge-base", "HEAD", ref],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                timeout=10,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()
    return None


def _git_show(path_rel: str, ref: str) -> str | None:
    try:
        result = subprocess.run(
            ["git", "show", f"{ref}:{path_rel}"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    return result.stdout


def validate_file(filename: str, base_commit: str | None) -> list[dict]:
    """Run every static rule against one sealed spec file, honoring
    `KNOWN_VIOLATIONS`. Returns the list of violations that are NOT in the
    allowlist (i.e. the ones that should fail this run).
    """
    path = SPEC_DIR / filename
    if not path.exists():
        return [{"rule": "file_missing", "step_id": "<file>", "message": f"file not found: {path}"}]

    steps = _steps_of(yaml.safe_load(path.read_text()))

    raw_violations: list[dict] = []
    raw_violations += expect_file_marker_violations(steps)
    raw_violations += known_bug_inversion_violations(steps)
    raw_violations += control_char_violations(steps, filename)

    if base_commit is not None:
        old_text = _git_show(f"tests/smoke-spec/{filename}", base_commit)
        if old_text is not None:
            old_steps = _steps_of(yaml.safe_load(old_text))
            raw_violations += additive_only_violations(old_steps, steps)

    violations = []
    for v in raw_violations:
        key = (filename, v["rule"], v["step_id"])
        if key in KNOWN_VIOLATIONS:
            print(f"KNOWN {filename:20s} [{v['rule']}] {v['step_id']}: {KNOWN_VIOLATIONS[key]}")
        else:
            violations.append(v)
    return violations


def check_coord_parsing() -> bool:
    """vimcode#1646's original check: each spec actually parses against the
    real driver it targets. Returns True (and prints SKIP) rather than
    failing the whole run when `coord` is not installed, since the static
    checks above are the ones that run without it; `main()`'s caller
    (CI) installs `code-coordinator` precisely so this one is not skipped
    there.
    """
    try:
        from coord.gtk_native_driver import parse_native_spec as parse_gtk_native_spec
        from coord.mac_native_driver import parse_native_spec as parse_mac_native_spec
        from coord.tui_pty_driver import parse_smoke_spec
        from coord.win_native_driver import parse_native_spec as parse_win_native_spec
    except ModuleNotFoundError as exc:
        print(
            f"SKIP coord driver parse check: could not import coord's driver "
            f"modules ({exc}).\n"
            "  Install the coordinator package to enable it: "
            "pip install code-coordinator",
            file=sys.stderr,
        )
        return True

    specs = [
        ("tui.yaml", parse_smoke_spec, "tui-pty"),
        ("win-gui.yaml", parse_win_native_spec, "win-native"),
        ("win-terminal.yaml", parse_win_native_spec, "win-native"),
        ("mac-gui.yaml", parse_mac_native_spec, "mac-native"),
        ("mac-gtk.yaml", parse_mac_native_spec, "mac-native"),
        ("gtk-gui.yaml", parse_gtk_native_spec, "gtk-native"),
    ]
    all_ok = True
    for filename, parser, kind in specs:
        path = SPEC_DIR / filename
        if not path.exists():
            print(f"FAIL {filename:20s} [{kind}] file not found: {path}")
            all_ok = False
            continue
        text = path.read_text()
        try:
            spec = parser(text)
        except Exception as exc:  # noqa: BLE001 — report every parse error, not just the first
            print(f"FAIL {filename:20s} [{kind}] {type(exc).__name__}: {exc}")
            all_ok = False
            continue
        step_ids = [s.step_id for s in spec.steps]
        print(f"OK   {filename:20s} [{kind}] {len(step_ids)} steps: {step_ids}")
    return all_ok


def main() -> int:
    all_ok = True

    base_commit = _resolve_base_commit()
    if base_commit is None:
        print(
            "NOTE: could not resolve a base ref ($SMOKE_SPEC_BASE_REF, "
            "origin/develop, develop) -- skipping the additive-only check "
            "(rule 4) for this run",
            file=sys.stderr,
        )

    print("-- static checks (unique markers / known_bug polarity / control chars / additive-only) --")
    for filename in SEALED_SPEC_FILES:
        violations = validate_file(filename, base_commit)
        if violations:
            all_ok = False
            for v in violations:
                print(f"FAIL {filename:20s} [{v['rule']}] {v['message']}")
        else:
            print(f"OK   {filename:20s} no static-check violations")

    print()
    print("-- coord driver parse check --")
    all_ok = check_coord_parsing() and all_ok

    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
