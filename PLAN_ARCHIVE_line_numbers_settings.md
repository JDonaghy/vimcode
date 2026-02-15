# Implementation Plan: Line Numbers & Settings Reload

**Goal:** Line numbers with settings.json configuration and runtime reload command.

**Status:** All steps complete ✅  
**Completed:** February 14, 2026  
**Dependencies:** None

---

## Overview

1. **Line numbers** — Absolute, relative, hybrid modes in gutter
2. **Settings** — JSON config at `~/.config/vimcode/settings.json`
3. **Reload** — `:config reload` to refresh settings at runtime

---

## Step 1: Settings Infrastructure ✅ COMPLETE

Created `Settings` struct with `LineNumberMode` enum, JSON load/save at `~/.config/vimcode/settings.json`. 5 tests added (151 total).

---

## Step 2: Line Number Rendering ✅ COMPLETE

Rendered line numbers in gutter with all modes (absolute/relative/hybrid), dynamic width, right-aligned, highlighted cursor line. 151 tests pass.

---

## Step 3: Settings Reload Command ✅ COMPLETE

Added `:config reload` command with error handling. Settings update at runtime without restart. 154 tests pass.

---

## Implementation Order

1. **Step 1:** Settings infrastructure ✅ COMPLETE
2. **Step 2:** Line number rendering (all modes) ✅ COMPLETE
3. **Step 3:** Settings reload command ✅ COMPLETE

---

## Success Criteria

- [x] Settings.json loads/saves at `~/.config/vimcode/settings.json`
- [x] Line numbers render in gutter (absolute/relative/hybrid)
- [x] Gutter width adjusts dynamically, current line highlighted
- [x] `:config reload` refreshes settings at runtime
- [x] Invalid JSON preserves current settings, shows error
- [x] All tests pass (154), no performance degradation
