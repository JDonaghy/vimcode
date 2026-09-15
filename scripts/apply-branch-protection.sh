#!/usr/bin/env bash
#
# #796: make CI a gate instead of an advisory signal.
#
# Applies the branch-protection settings declared in .github/branch-protection.json
# to this repository, so a red CI run actually blocks a merge. Before #796 the
# two CI jobs ran on every PR and push but nothing enforced them: `develop` had
# no protection object at all and `main` had one with no `required_status_checks`
# key, so a PR with red checks was mergeable by clicking the button.
#
# The settings live in a versioned JSON file rather than only in the repo's web
# UI for two reasons: they are reviewable in a diff, and tests/branch_protection.rs
# can assert the required check names still match the job names in
# .github/workflows/ci.yml. A required context naming a job that no longer
# exists is never reported by GitHub, so it stays forever-pending and blocks
# every pull request -- a rename is the failure mode most likely to wedge this.
#
# Usage:
#   scripts/apply-branch-protection.sh              # apply, then verify
#   scripts/apply-branch-protection.sh --check      # audit only, no writes
#   scripts/apply-branch-protection.sh --dry-run    # print payloads, no network
#   scripts/apply-branch-protection.sh --repo OWNER/NAME
#
# Requires: python3 (rendering/comparing JSON), and for --check/apply an
# authenticated `gh` with admin rights on the repo. --dry-run needs neither gh
# nor network access.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG="$REPO_ROOT/.github/branch-protection.json"

MODE="apply"
REPO_SLUG=""

usage() {
    sed -n '3,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --check)   MODE="check";   shift ;;
        --dry-run) MODE="dry-run"; shift ;;
        --repo)    REPO_SLUG="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if ! command -v python3 >/dev/null 2>&1; then
    echo "error: python3 is required (used to render and compare JSON)" >&2
    exit 1
fi

if [ ! -f "$CONFIG" ]; then
    echo "error: missing $CONFIG" >&2
    exit 1
fi

# Branch list comes from the config, so adding a branch there is the only edit
# needed to protect it.
branches() {
    python3 -c '
import json, sys
cfg = json.load(open(sys.argv[1]))
for name in cfg["branches"]:
    print(name)
' "$CONFIG"
}

# Render the exact JSON body PUT to
# /repos/{owner}/{repo}/branches/{branch}/protection. `_comment` keys are
# documentation for humans reading the config and are stripped here.
payload_for() {
    python3 -c '
import json, sys
cfg = json.load(open(sys.argv[1]))
branch = cfg["branches"][sys.argv[2]]
print(json.dumps({
    "required_status_checks": {
        "strict": branch["strict"],
        "contexts": cfg["required_contexts"],
    },
    "enforce_admins": branch["enforce_admins"],
    "required_pull_request_reviews": None,
    "restrictions": None,
    "allow_force_pushes": False,
    "allow_deletions": False,
}, indent=2))
' "$CONFIG" "$1"
}

resolve_repo() {
    if [ -n "$REPO_SLUG" ]; then
        echo "$REPO_SLUG"
        return
    fi
    local url
    url="$(git -C "$REPO_ROOT" remote get-url origin 2>/dev/null || true)"
    if [ -z "$url" ]; then
        echo "error: no 'origin' remote; pass --repo OWNER/NAME" >&2
        exit 1
    fi
    # git@github.com:OWNER/NAME.git | https://github.com/OWNER/NAME(.git)
    url="${url%.git}"
    url="${url##*github.com[:/]}"
    echo "$url"
}

# Compare live protection against the config. Prints a human-readable drift
# report; exits non-zero when anything differs (so --check is CI/cron usable).
compare() {
    python3 -c '
import json, sys
cfg = json.load(open(sys.argv[1]))
branch = sys.argv[2]
want = cfg["branches"][branch]
raw = sys.stdin.read().strip()
try:
    live = json.loads(raw) if raw else {}
except json.JSONDecodeError:
    live = {}

checks = live.get("required_status_checks") or {}
live_contexts = sorted(checks.get("contexts") or [])
want_contexts = sorted(cfg["required_contexts"])

drift = []
if live_contexts != want_contexts:
    drift.append(f"  required contexts: live={live_contexts} want={want_contexts}")
if checks.get("strict") != want["strict"]:
    drift.append(f"  strict: live={checks.get('strict')!r} want={want['strict']!r}")
live_admins = (live.get("enforce_admins") or {}).get("enabled")
if live_admins != want["enforce_admins"]:
    drift.append(f"  enforce_admins: live={live_admins!r} want={want['enforce_admins']!r}")

if drift:
    print(f"DRIFT {branch}")
    print("\n".join(drift))
    sys.exit(1)
print(f"OK    {branch}: contexts={want_contexts} strict={want['strict']} "
      f"enforce_admins={want['enforce_admins']}")
' "$CONFIG" "$1"
}

if [ "$MODE" = "dry-run" ]; then
    for b in $(branches); do
        echo "=== $b ==="
        payload_for "$b"
    done
    exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
    echo "error: gh is required for --check/apply (use --dry-run to preview)" >&2
    exit 1
fi

REPO="$(resolve_repo)"
status=0

for b in $(branches); do
    if [ "$MODE" = "apply" ]; then
        echo "applying protection to $REPO@$b ..."
        payload_for "$b" | gh api -X PUT "repos/$REPO/branches/$b/protection" \
            --input - >/dev/null
    fi
    # Both modes end by reading back the live settings: apply verifies what it
    # just wrote rather than trusting a 200.
    live="$(gh api "repos/$REPO/branches/$b/protection" 2>/dev/null || echo '{}')"
    if ! printf '%s' "$live" | compare "$b"; then
        status=1
    fi
done

exit "$status"
