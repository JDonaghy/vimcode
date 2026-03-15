# VimCode Project State

**Last updated:** Mar 14, 2026 (Session 181 — LaTeX Extension: Tree-sitter + Spell Checking) | **Tests:** 4331

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 180b are in **SESSION_HISTORY.md**.

---

## Testing Policy

**Every new Vim feature and every bug fix MUST have comprehensive integration tests before the work is considered done.** Subtle bugs (register content, cursor position, newline handling, linewise vs. char-mode paste) are only reliably caught by tests. The process is:

1. Write failing tests that document the expected Vim behavior
2. Implement/fix the feature until all tests pass
3. Run the full suite (`cargo test`) — no regressions allowed

When implementing a new key/command, add tests covering:
- Basic happy path
- Edge cases: start/middle/end of line, start/end of file, empty buffer, count prefix
- Register content (text and `is_linewise` flag)
- Cursor position after the operation
- Interaction with paste (`p`/`P`) to verify the yanked/deleted content behaves correctly

---

## Recent Work

### Session 181 — LaTeX Extension: Tree-sitter Syntax + Spell Checking (Mar 14, 2026)
- **Tree-sitter LaTeX support**: Added `SyntaxLanguage::Latex` — 18th built-in language. Vendored `tree-sitter-latex` v0.3.0 grammar (language version 14, compatible with tree-sitter 0.24) under `vendor/tree-sitter-latex/`. Compiled via `build.rs` + `cc` crate. Highlight query covers comments, commands (`generic_command`), sections, math (inline/displayed/environment), labels, citations. Breadcrumb scopes: `generic_environment`, `section`, `chapter`, `subsection`, `subsubsection`.
- **File extensions**: `.tex`, `.bib`, `.cls`, `.sty`, `.dtx`, `.ltx`
- **LaTeX-aware spell checking**: Changed `check_line()` signature from `has_syntax: bool` to `syntax_lang: Option<SyntaxLanguage>`. LaTeX mode inverts the logic — checks all prose text EXCEPT words in `keyword` (commands) and `type` (math) scopes. Added `is_in_latex_command_or_math()` helper.
- **Flatpak**: Regenerated `flatpak/cargo-sources.json` with new `cc` and `tree-sitter-language` dependencies.
- 15 new tests across syntax.rs (8: detection + highlighting) and spell.rs (3: LaTeX prose/commands/math + existing tests updated for new API) — 4331 total

> Sessions 180b and earlier archived in **SESSION_HISTORY.md**.
