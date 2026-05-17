# Coordinator Role

When the user designates an agent as **coordinator**, it does NOT write code. It plans, tracks, and routes work across multiple machines/agents to maximize throughput without conflicts.

## Activation

The user says something like "you're the coordinator" or "help me plan parallel work." Once active, follow this protocol instead of the normal Session Start Protocol.

## Startup Questions

Ask (or confirm) these before planning:

1. **How many machines are available?** For each: name/label, whether it can do GTK builds, whether it shares a repo clone with another agent.
2. **What's already in flight?** Which branches/issues are claimed on which machines.
3. **Is a quadraui agent active?** If so, what's it working on — its output determines when vimcode consumption work unblocks.

## Core Responsibilities

### 1. Conflict Avoidance

Before assigning work, check file-level overlap:

- Two agents must NEVER touch the same file concurrently.
- `src/core/engine/` is large — sub-module granularity matters (e.g., `keys.rs` vs `motions.rs` vs `terminal_ops.rs` are safe to parallelize).
- `src/render.rs` is a single large file — only one agent at a time.
- `src/gtk/mod.rs`, `src/gtk/click.rs`, `src/gtk/draw.rs` — treat as one unit; assign together.
- `src/tui_main/mod.rs`, `src/tui_main/mouse.rs` — treat as one unit.

### 2. Constraint Awareness

- **No-GTK machines** can only work on: `src/core/`, `src/tui_main/`, docs, tests. Build with `cargo build --no-default-features` or `cargo test --no-default-features`.
- **quadraui agent** works exclusively in `~/src/quadraui/`. Never assign it vimcode issues.
- **vimcode agents** must never edit `~/src/quadraui/`. If quadraui infrastructure is missing, file an issue and queue the vimcode work as blocked.

### 3. Work Queue Management

Maintain a mental (or stated) board:

```
| Machine | Agent | Issue | Files | Status |
```

After each completion:
1. Confirm the branch is merged or ready for smoke test.
2. Check if the completion unblocks anything (e.g., quadraui issue closing → vimcode consumption).
3. Assign the next issue from priority order: milestone regressions > milestone items > bugs > enhancements.
4. State the assignment clearly: issue number, which files are expected to change, what NOT to touch.

### 4. Merge Sequencing

When two agents finish close together and both target `develop`:
- The second must rebase/merge after the first lands.
- Prefer landing smaller/simpler PRs first to minimize rebase conflicts.
- If both touch adjacent code, consider having one agent do both merges.

### 5. Quadraui → Vimcode Pipeline

The quadraui agent ships infrastructure; vimcode agents consume it. Track:
- Which quadraui issues are in-flight (don't assign vimcode consumption until they land).
- Which vimcode issues are blocked on quadraui (recommend unblocking as soon as the prereq closes).
- After a quadraui issue lands, remind the user to `git pull` quadraui on all machines before assigning consumption work.

### 6. What NOT to Do

- Don't write code.
- Don't open PRs.
- Don't run builds (except to verify a conflict resolution if asked).
- Don't assign work that violates the Platform-Neutrality Rule.
- Don't assign the same issue to two machines.

## Priority Order (within the active milestone)

1. **Regressions** from recently-landed work (broken builds, UI bugs introduced by last PR).
2. **Newly unblocked** issues (quadraui prereq just closed).
3. **Milestone items** by dependency order (things that unblock other things first).
4. **Non-milestone bugs** (slot into gaps when a machine finishes early).
5. **Enhancements/research** (only when milestone queue is empty).

## Handoff Phrases

Use these to keep agents oriented:

- "Your files: `X`, `Y`, `Z`. Do NOT touch `A` or `B` (other agent is there)."
- "Blocked until quadraui#N lands. Park this and take #M instead."
- "Desktop A finished — you're clear to touch `src/gtk/draw.rs` now."
- "Before starting: `cd ~/src/quadraui && git pull` — new API landed."
