#!/usr/bin/env python3
"""End-to-end vimcode release with quadraui dependency update.

Every vimcode release that bumps quadraui (the common case) requires the
quadraui crate to be published first.  This script drives both halves —
quadraui publish, then vimcode release — as a guided, resumable workflow.

Usage:

    python3 scripts/release.py --quadraui-version 0.1.9 --vimcode-version 0.16.0

The script automates the safe/mechanical parts and pauses for confirmation
before anything irreversible (push, merge, cargo publish).  It is resumable:
re-running after an interruption detects which steps are already done and
picks up where it left off.

Phases:

  1. Preflight — both repos clean, vimcode on develop, versions valid
  2. quadraui prepare — bump version, retitle CHANGELOG, commit on release branch
  3. quadraui publish — push, PR, merge to develop, promote to main, wait for
     cargo publish (CI-driven)
  4. vimcode update — cargo update, bump version, retitle CHANGELOG, commit
  5. vimcode gate — run platform-conformance.sh
  6. vimcode release — develop→main PR, merge triggers release.yml

Pass --skip-quadraui when releasing vimcode without a quadraui change (rare).
Pass --skip-gate to skip the platform-conformance run (when already done).
"""
from __future__ import annotations

import argparse
import datetime
import re
import subprocess
import sys
import time
from pathlib import Path

VIMCODE_DIR = Path(__file__).resolve().parent.parent
QUADRAUI_DIR = VIMCODE_DIR.parent / "quadraui"

VERSION_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
UNRELEASED_RE = re.compile(r"(?m)^## \[Unreleased\]\s*$")
RELEASE_HEADER_RE = re.compile(r"(?m)^## \[(\d+\.\d+\.\d+)\][^\n]*$")

BOLD = "\033[1m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"
RESET = "\033[0m"


def banner(phase: str, msg: str) -> None:
    print(f"\n{BOLD}{'=' * 60}{RESET}")
    print(f"{BOLD}  [{phase}] {msg}{RESET}")
    print(f"{BOLD}{'=' * 60}{RESET}\n")


def info(msg: str) -> None:
    print(f"  {GREEN}✓{RESET} {msg}")


def warn(msg: str) -> None:
    print(f"  {YELLOW}⚠{RESET} {msg}")


def fail(msg: str) -> None:
    print(f"\n  {RED}✗ {msg}{RESET}\n", file=sys.stderr)
    sys.exit(1)


AUTO_YES = False


def confirm(prompt: str) -> bool:
    if AUTO_YES:
        print(f"\n  {YELLOW}?{RESET} {prompt} [y/N] y (--yes)")
        return True
    try:
        answer = input(f"\n  {YELLOW}?{RESET} {prompt} [y/N] ").strip().lower()
    except EOFError:
        warn("no TTY — use --yes for non-interactive mode")
        return True
    return answer in ("y", "yes")


def run(cmd: list[str], cwd: Path, check: bool = True,
        capture: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd, cwd=cwd, capture_output=capture, text=True, check=check,
    )


def run_show(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess:
    """Run with output visible to the user."""
    return subprocess.run(cmd, cwd=cwd, text=True)


def git(args: list[str], cwd: Path, check: bool = True) -> subprocess.CompletedProcess:
    return run(["git", *args], cwd, check=check)


def git_clean(cwd: Path) -> bool:
    result = git(["status", "--porcelain", "--untracked-files=no"], cwd)
    return result.stdout.strip() == ""


def git_branch(cwd: Path) -> str:
    return git(["rev-parse", "--abbrev-ref", "HEAD"], cwd).stdout.strip()


def parse_version(v: str) -> tuple[int, int, int]:
    m = VERSION_RE.match(v.strip())
    if not m:
        fail(f"invalid version: {v!r}")
        raise SystemExit(1)  # unreachable, for type checker
    return int(m.group(1)), int(m.group(2)), int(m.group(3))


def read_cargo_version(cargo_toml: Path) -> str:
    text = cargo_toml.read_text()
    m = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not m:
        fail(f"no version found in {cargo_toml}")
        raise SystemExit(1)
    return m.group(1)


def set_cargo_version(cargo_toml: Path, new_ver: str) -> None:
    text = cargo_toml.read_text()
    new_text = re.sub(
        r'(?m)^(version\s*=\s*)"[^"]+"', rf'\1"{new_ver}"', text, count=1,
    )
    cargo_toml.write_text(new_text)


def read_lock_version(lock_path: Path, pkg: str) -> str:
    text = lock_path.read_text()
    pattern = re.compile(
        rf'(?m)^\[\[package\]\]\s*\nname\s*=\s*"{re.escape(pkg)}"\s*\n'
        r'version\s*=\s*"([^"]+)"'
    )
    m = pattern.search(text)
    if not m:
        fail(f"{pkg} not found in {lock_path}")
        raise SystemExit(1)
    return m.group(1)


def retitle_changelog(path: Path, new_ver: str, date: str) -> None:
    text = path.read_text()
    matches = list(UNRELEASED_RE.finditer(text))
    if len(matches) != 1:
        fail(f"expected 1 [Unreleased] header in {path}, found {len(matches)}")
    m = matches[0]
    replacement = f"## [Unreleased]\n\n## [{new_ver}] - {date}"
    path.write_text(text[: m.start()] + replacement + text[m.end() :])


def unreleased_body(path: Path) -> str:
    text = path.read_text()
    m = UNRELEASED_RE.search(text)
    if not m:
        return ""
    rest = text[m.end():]
    nxt = re.search(r"(?m)^## \[", rest)
    return (rest[: nxt.start()] if nxt else rest).strip()


def tag_exists_on_remote(tag: str, cwd: Path) -> bool:
    r = git(["ls-remote", "--tags", "origin", f"refs/tags/{tag}"], cwd, check=False)
    return tag in r.stdout


def wait_for_crate(pkg: str, version: str, timeout: int = 600) -> bool:
    """Poll crates.io until the version appears (or timeout)."""
    import urllib.request
    import json
    url = f"https://crates.io/api/v1/crates/{pkg}/versions"
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "vimcode-release/1.0"})
            with urllib.request.urlopen(req, timeout=15) as resp:
                data = json.loads(resp.read())
                versions = [v["num"] for v in data.get("versions", [])]
                if version in versions:
                    return True
        except Exception:
            pass
        print(f"    waiting for {pkg} {version} on crates.io...", flush=True)
        time.sleep(15)
    return False


# ── Phase 1: Preflight ───────────────────────────────────────────────


def preflight(args: argparse.Namespace) -> None:
    banner("1/6", "Preflight checks")

    if not VIMCODE_DIR.joinpath("Cargo.toml").exists():
        fail(f"not a vimcode repo: {VIMCODE_DIR}")

    if git_branch(VIMCODE_DIR) != "develop":
        fail("vimcode must be on the develop branch")

    if not git_clean(VIMCODE_DIR):
        fail("vimcode working tree is dirty — commit or stash first")

    cur_vc = read_cargo_version(VIMCODE_DIR / "Cargo.toml")
    if cur_vc == args.vimcode_version:
        info(f"vimcode already at {args.vimcode_version} (resuming)")
    elif parse_version(args.vimcode_version) <= parse_version(cur_vc):
        fail(f"vimcode version {args.vimcode_version} is not greater than current {cur_vc}")
    else:
        info(f"vimcode: {cur_vc} → {args.vimcode_version}")

    if not args.skip_quadraui:
        if not QUADRAUI_DIR.joinpath("Cargo.toml").exists():
            fail(f"quadraui repo not found at {QUADRAUI_DIR}")
        if not git_clean(QUADRAUI_DIR):
            fail("quadraui working tree is dirty — commit or stash first")

        cur_qu = read_cargo_version(QUADRAUI_DIR / "quadraui" / "Cargo.toml")
        if parse_version(args.quadraui_version) <= parse_version(cur_qu):
            fail(f"quadraui version {args.quadraui_version} is not greater than current {cur_qu}")
        info(f"quadraui: {cur_qu} → {args.quadraui_version}")

        body = unreleased_body(QUADRAUI_DIR / "CHANGELOG.md")
        if not body:
            fail("quadraui CHANGELOG.md [Unreleased] section is empty")
        info("quadraui CHANGELOG has unreleased entries")

    body = unreleased_body(VIMCODE_DIR / "CHANGELOG.md")
    if not body:
        warn("vimcode CHANGELOG.md [Unreleased] section is empty — add entries before releasing")
    else:
        info("vimcode CHANGELOG has unreleased entries")

    info("preflight passed")


# ── Phases 2-3: quadraui release (delegated to release_full.py) ──────


def quadraui_release(args: argparse.Namespace) -> None:
    banner("2-3/6", f"Release quadraui {args.quadraui_version}")

    release_script = QUADRAUI_DIR / "scripts" / "release_full.py"
    if not release_script.exists():
        fail(f"quadraui release script not found: {release_script}")

    qu_ver = args.quadraui_version
    tag = f"v{qu_ver}"

    if tag_exists_on_remote(tag, QUADRAUI_DIR):
        info(f"quadraui {tag} already tagged on remote")
        if wait_for_crate("quadraui", qu_ver, timeout=10):
            info(f"quadraui {qu_ver} confirmed on crates.io")
            return
        info("tag exists but crate not yet on crates.io — waiting...")

    cmd = [sys.executable, str(release_script), qu_ver]
    if AUTO_YES:
        cmd.append("--yes")

    result = run_show(cmd, QUADRAUI_DIR)
    if result.returncode != 0:
        fail(f"quadraui release failed (exit {result.returncode})")

    info(f"quadraui {qu_ver} released")


# ── Phase 4: vimcode update ──────────────────────────────────────────


def vimcode_update(args: argparse.Namespace) -> None:
    banner("4/6", f"Update vimcode to {args.vimcode_version}")

    if git_branch(VIMCODE_DIR) != "develop":
        git(["checkout", "develop"], VIMCODE_DIR)
    pull = git(["pull", "--ff-only"], VIMCODE_DIR, check=False)
    if pull.returncode != 0:
        ahead = git(["rev-list", "--count", "origin/develop..HEAD"], VIMCODE_DIR)
        if int(ahead.stdout.strip()) > 0:
            info(f"develop has diverged ({ahead.stdout.strip()} local commits ahead) — rebasing")
            git(["rebase", "origin/develop"], VIMCODE_DIR)
        else:
            fail(f"git pull --ff-only failed:\n{pull.stderr}")

    old_locked = read_lock_version(VIMCODE_DIR / "Cargo.lock", "quadraui")
    if not args.skip_quadraui:
        qu_ver = args.quadraui_version
        if old_locked == qu_ver:
            info(f"quadraui already locked to {qu_ver}")
        else:
            info(f"updating quadraui lock: {old_locked} → {qu_ver}")
            result = run(["cargo", "update", "-p", "quadraui"], VIMCODE_DIR, check=False)
            if result.returncode != 0:
                fail(f"cargo update -p quadraui failed:\n{result.stderr}")

            new_locked = read_lock_version(VIMCODE_DIR / "Cargo.lock", "quadraui")
            if new_locked != qu_ver:
                fail(
                    f"cargo update resolved quadraui to {new_locked}, "
                    f"expected {qu_ver} — is the version requirement compatible?"
                )
            info(f"quadraui locked to {new_locked}")
    else:
        info(f"updating quadraui lock (currently {old_locked})")
        result = run(["cargo", "update", "-p", "quadraui"], VIMCODE_DIR, check=False)
        if result.returncode != 0:
            fail(f"cargo update -p quadraui failed:\n{result.stderr}")
        new_locked = read_lock_version(VIMCODE_DIR / "Cargo.lock", "quadraui")
        if new_locked != old_locked:
            info(f"quadraui updated: {old_locked} → {new_locked}")
        else:
            info(f"quadraui stays at {old_locked}")

    cur_vc = read_cargo_version(VIMCODE_DIR / "Cargo.toml")
    vc_ver = args.vimcode_version
    if cur_vc == vc_ver:
        info(f"vimcode already at {vc_ver}")
    else:
        set_cargo_version(VIMCODE_DIR / "Cargo.toml", vc_ver)
        info(f"bumped Cargo.toml: {cur_vc} → {vc_ver}")

    run(["cargo", "update", "-p", "vimcode", "--offline"], VIMCODE_DIR, check=False)

    changelog = VIMCODE_DIR / "CHANGELOG.md"
    text = changelog.read_text()
    if f"## [{vc_ver}]" in text:
        info(f"CHANGELOG already has [{vc_ver}] section")
    else:
        retitle_changelog(changelog, vc_ver, datetime.date.today().isoformat())
        info(f"retitled CHANGELOG [Unreleased] → [{vc_ver}]")

    if not git_clean(VIMCODE_DIR):
        print("\n  Running quick test (--no-default-features)...")
        test_result = run_show(
            ["cargo", "test", "--no-default-features", "-q"],
            VIMCODE_DIR,
        )
        if test_result.returncode != 0:
            fail("cargo test failed — fix before continuing")
        info("tests passed")

        files = ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"]
        git(["add"] + files, VIMCODE_DIR)
        new_qu = read_lock_version(VIMCODE_DIR / "Cargo.lock", "quadraui")
        qu_note = f", quadraui {new_qu}" if new_qu != old_locked else ""
        git(
            ["commit", "-m",
             f"chore(release): bump vimcode to v{vc_ver}{qu_note}"],
            VIMCODE_DIR,
        )
        info("committed version bump")
    else:
        info("nothing to commit (already done)")


# ── Phase 5: Gate ────────────────────────────────────────────────────


def vimcode_gate(args: argparse.Namespace) -> None:
    banner("5/6", "Platform conformance gate")

    if args.skip_gate:
        warn("gate skipped (--skip-gate)")
        return

    gate_script = VIMCODE_DIR / "scripts" / "platform-conformance.sh"
    if not gate_script.exists():
        fail(f"gate script not found: {gate_script}")

    print("  Running scripts/platform-conformance.sh ...\n")
    result = run_show(["bash", str(gate_script)], VIMCODE_DIR)
    if result.returncode != 0:
        fail(f"gate failed with exit {result.returncode} — see output above")
    info("gate passed")


# ── Phase 6: Release ─────────────────────────────────────────────────


def reconcile_main_into_develop() -> None:
    """Merge origin/main into develop so the PR has no conflicts."""
    git(["fetch", "origin", "main"], VIMCODE_DIR)
    behind = git(
        ["rev-list", "--count", "HEAD..origin/main"], VIMCODE_DIR,
    ).stdout.strip()
    if behind == "0":
        info("develop already contains all of main")
        return

    info(f"main is {behind} commit(s) ahead of develop — merging")
    merge = git(["merge", "origin/main", "--no-edit"], VIMCODE_DIR, check=False)
    if merge.returncode != 0:
        conflicts = git(
            ["diff", "--name-only", "--diff-filter=U"], VIMCODE_DIR,
        ).stdout.strip()
        if conflicts:
            # For version-only conflicts in Cargo.toml/Cargo.lock, take ours
            version_files = {"Cargo.toml", "Cargo.lock"}
            conflict_set = set(conflicts.splitlines())
            auto_resolvable = conflict_set <= version_files
            if auto_resolvable or conflict_set - version_files == {"CHANGELOG.md"}:
                for f in conflict_set:
                    git(["checkout", "--ours", f], VIMCODE_DIR)
                    git(["add", f], VIMCODE_DIR)
                git(["commit", "--no-edit"], VIMCODE_DIR)
                info("auto-resolved version conflicts (kept develop's versions)")
            else:
                fail(
                    f"merge conflicts in unexpected files: {conflicts}\n"
                    "  Resolve manually: git merge origin/main"
                )
        else:
            fail(f"merge failed:\n{merge.stderr}")


def vimcode_release(args: argparse.Namespace) -> None:
    banner("6/6", f"Release vimcode v{args.vimcode_version}")

    vc_ver = args.vimcode_version

    reconcile_main_into_develop()

    git(["push", "origin", "develop"], VIMCODE_DIR, check=False)
    info("develop pushed")

    changelog = VIMCODE_DIR / "CHANGELOG.md"
    text = changelog.read_text()
    header = f"## [{vc_ver}]"
    body_lines: list[str] = []
    in_section = False
    for line in text.splitlines():
        if line.startswith(header):
            in_section = True
            continue
        if in_section and line.startswith("## ["):
            break
        if in_section:
            body_lines.append(line)
    release_body = "\n".join(body_lines).strip()

    pr_body = f"## Release v{vc_ver}\n\n{release_body}"
    if not args.skip_quadraui:
        pr_body += f"\n\nIncludes quadraui {args.quadraui_version}."

    print(f"\n  About to create the develop → main PR for v{vc_ver}.")
    print(f"  Merging it triggers release.yml which builds + publishes.\n")

    # Check for an existing develop→main PR
    existing = run(
        ["gh", "pr", "list", "--base", "main", "--head", "develop",
         "--json", "url,number", "--jq", ".[0]"],
        VIMCODE_DIR, check=False,
    )
    pr_url = None
    if existing.returncode == 0 and existing.stdout.strip():
        import json
        pr_info = json.loads(existing.stdout.strip())
        pr_url = pr_info["url"]
        info(f"updating existing PR #{pr_info['number']}: {pr_url}")
        run(
            ["gh", "pr", "edit", str(pr_info["number"]),
             "--title", f"Release v{vc_ver}", "--body", pr_body],
            VIMCODE_DIR, check=False,
        )
    else:
        if not confirm("Create the release PR?"):
            print(f"\n  Create it manually:")
            print(f'    gh pr create --base main --head develop --title "Release v{vc_ver}"')
            sys.exit(0)

        pr_result = run(
            ["gh", "pr", "create",
             "--base", "main",
             "--head", "develop",
             "--title", f"Release v{vc_ver}",
             "--body", pr_body],
            VIMCODE_DIR,
            check=False,
        )
        if pr_result.returncode == 0:
            pr_url = pr_result.stdout.strip()
            info(f"release PR created: {pr_url}")
        else:
            warn(f"PR creation failed — check GitHub\n{pr_result.stderr}")

    print(f"\n  {YELLOW}Final steps:{RESET}")
    print(f"    1. Review the PR, confirm CI is green")
    print(f"    2. Merge with a MERGE COMMIT (not squash/rebase)")
    if pr_url:
        print(f"       gh pr merge --merge {pr_url}")
    print(f"    3. release.yml builds + publishes the GitHub Release")
    print(f"    4. Verify: download artifacts, run --version, check quadraui version")

    info(f"release v{vc_ver} ready for merge")


# ── Main ─────────────────────────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(
        description="End-to-end vimcode release with quadraui dependency update.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--quadraui-version",
        help="new quadraui version to publish (e.g. 0.1.9)",
    )
    parser.add_argument(
        "--vimcode-version", required=True,
        help="new vimcode version (e.g. 0.16.0)",
    )
    parser.add_argument(
        "--skip-quadraui", action="store_true",
        help="skip quadraui publish (release vimcode only)",
    )
    parser.add_argument(
        "--skip-gate", action="store_true",
        help="skip platform-conformance gate (already run)",
    )
    parser.add_argument(
        "--yes", "-y", action="store_true",
        help="answer yes to all prompts (non-interactive mode)",
    )
    args = parser.parse_args()

    if args.yes:
        global AUTO_YES
        AUTO_YES = True

    if not args.skip_quadraui and not args.quadraui_version:
        parser.error("--quadraui-version is required (or pass --skip-quadraui)")

    preflight(args)
    if not args.skip_quadraui:
        quadraui_release(args)
    vimcode_update(args)
    vimcode_gate(args)
    vimcode_release(args)


if __name__ == "__main__":
    main()
