#!/usr/bin/env python3
"""Tests for `tools/comment_history_lint.py` (#1896).

Stdlib-only (`unittest`): no pytest dependency just to cover one script. Run
with:

    python3 tools/test_comment_history_lint.py
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import comment_history_lint as lint  # noqa: E402


class CommentTextTest(unittest.TestCase):
    def test_rust_line_comment_is_found(self):
        self.assertEqual(lint.comment_text("let x = 1; // hello", ".rs"), "// hello")

    def test_rust_outer_doc_comment_is_found(self):
        self.assertEqual(lint.comment_text("/// does a thing", ".rs"), "/// does a thing")

    def test_rust_inner_doc_comment_is_found(self):
        self.assertEqual(lint.comment_text("//! module overview", ".rs"), "//! module overview")

    def test_rust_url_inside_comment_does_not_truncate_it(self):
        line = "// see https://example.com/foo for details"
        self.assertEqual(lint.comment_text(line, ".rs"), line[line.index("//") :])

    def test_rust_code_only_line_has_no_comment(self):
        self.assertIsNone(lint.comment_text("let x = 1;", ".rs"))

    def test_non_rs_suffix_returns_none(self):
        self.assertIsNone(lint.comment_text("// hello", ".md"))


class ScanFileTest(unittest.TestCase):
    def _scan(self, text: str, suffix: str = ".rs") -> list[lint.Finding]:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / f"sample{suffix}"
            path.write_text(text)
            return lint.scan_file(path)

    def test_issue_reference_is_flagged(self):
        findings = self._scan("// workaround for #1234\nfn f() {}\n")
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].is_issue_ref)
        self.assertEqual(findings[0].phrases, [])

    def test_cross_repo_issue_reference_is_flagged(self):
        findings = self._scan("// see quadraui#42 for background\nfn f() {}\n")
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].is_issue_ref)

    def test_single_digit_bare_ref_is_not_flagged(self):
        # #\d{2,5}: a lone "#1" is too short to reliably be an issue ref.
        findings = self._scan("// case #1 of the enum\nfn f() {}\n")
        self.assertEqual(findings, [])

    def test_history_phrase_is_flagged_case_insensitively(self):
        findings = self._scan("// This Used To return None\nfn f() {}\n")
        self.assertEqual(len(findings), 1)
        self.assertFalse(findings[0].is_issue_ref)
        self.assertEqual(findings[0].phrases, ["used to"])

    def test_plain_comment_is_not_flagged(self):
        findings = self._scan("// returns the current value\nfn f() {}\n")
        self.assertEqual(findings, [])

    def test_both_issue_ref_and_phrase_on_one_line_is_one_finding(self):
        findings = self._scan("// no longer needed, was added for #42\n")
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].is_issue_ref)
        self.assertEqual(findings[0].phrases, ["no longer"])

    def test_non_comment_line_with_hash_like_code_is_ignored(self):
        # Rust has no '#' comment marker, so a line with a literal '#'
        # outside any '//' must not be flagged.
        findings = self._scan('let s = "#1234 not a comment";\n')
        self.assertEqual(findings, [])

    def test_todo_paren_ref_is_not_flagged(self):
        findings = self._scan("// TODO(#1234): remove this once the issue closes\n")
        self.assertEqual(findings, [])

    def test_fixme_paren_ref_is_not_flagged(self):
        findings = self._scan("// FIXME(#99): tighten this bound\n")
        self.assertEqual(findings, [])

    def test_todo_paren_ref_does_not_hide_a_second_bare_ref_on_the_same_line(self):
        findings = self._scan("// TODO(#1234): same root cause as #5678\n")
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].is_issue_ref)

    def test_doc_comment_issue_ref_is_flagged(self):
        findings = self._scan("/// workaround for #4321\nfn f() {}\n")
        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].is_issue_ref)

    def test_fenced_doc_comment_code_is_not_flagged(self):
        findings = self._scan(
            "/// before the fence\n"
            "/// ```ignore\n"
            "/// // used to call #1234 here\n"
            "/// ```\n"
            "fn f() {}\n"
        )
        self.assertEqual(findings, [])

    def test_fenced_doc_comment_only_skips_until_closing_fence(self):
        findings = self._scan(
            "/// ```text\n"
            "/// see #1234 inside the fence\n"
            "/// ```\n"
            "/// no longer true, after the fence\n"
        )
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].phrases, ["no longer"])

    def test_unclosed_fence_resets_at_end_of_comment_block(self):
        # A doc comment that never closes its fence before code resumes is a
        # malformed doctest; the lint should not silently stay "inside a
        # fence" forever and swallow later real comments in the file.
        findings = self._scan(
            "/// ```text\n"
            "/// still fenced\n"
            "fn f() {}\n"
            "// used to be different\n"
        )
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].phrases, ["used to"])


class GroupForTest(unittest.TestCase):
    def test_top_level_src_file_is_src_group(self):
        root = Path("/repo")
        path = root / "src" / "lib.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "src")

    def test_app_subdir_is_its_own_group(self):
        root = Path("/repo")
        path = root / "src" / "app" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "src/app")

    def test_render_subdir_is_its_own_group(self):
        root = Path("/repo")
        path = root / "src" / "render" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "src/render")

    def test_core_engine_subdir_is_its_own_group(self):
        root = Path("/repo")
        path = root / "src" / "core" / "engine" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "src/core/engine")

    def test_core_non_engine_file_is_core_group(self):
        root = Path("/repo")
        path = root / "src" / "core" / "buffer.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "src/core")

    def test_gtk_backend_is_backends_group(self):
        root = Path("/repo")
        path = root / "src" / "gtk" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "backends")

    def test_tui_main_backend_is_backends_group(self):
        root = Path("/repo")
        path = root / "src" / "tui_main" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "backends")

    def test_win_backend_is_backends_group(self):
        root = Path("/repo")
        path = root / "src" / "win" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "backends")

    def test_macos_backend_is_backends_group(self):
        root = Path("/repo")
        path = root / "src" / "macos" / "mod.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "backends")

    def test_harness_backend_is_backends_group(self):
        root = Path("/repo")
        path = root / "src" / "harness" / "plugin_panel.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "backends")

    def test_nested_harness_file_is_still_backends_group(self):
        root = Path("/repo")
        path = root / "src" / "harness" / "plugin_panel" / "tests.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "backends")

    def test_tests_dir_file_is_tests_group(self):
        root = Path("/repo")
        path = root / "tests" / "smoke_catalogue.rs"
        self.assertEqual(lint.group_for(path, repo_root=root), "tests")

    def test_file_outside_src_and_tests_is_other(self):
        root = Path("/repo")
        path = root / "tools" / "comment_history_lint.py"
        self.assertEqual(lint.group_for(path, repo_root=root), "other")


class FixBodyTest(unittest.TestCase):
    def test_leading_issue_tag_is_stripped_and_capitalized(self):
        body = "#1630 review: tightened the bound check"
        self.assertEqual(lint.fix_body(body), "Tightened the bound check")

    def test_ref_only_body_is_deleted(self):
        self.assertIsNone(lint.fix_body("see #883"))

    def test_todo_paren_ref_is_left_unchanged(self):
        body = "TODO(#1234): remove this once the issue closes"
        self.assertEqual(lint.fix_body(body), body)

    def test_safety_comment_is_left_unchanged(self):
        body = "SAFETY: caller guarantees the pointer in #42 is valid"
        self.assertEqual(lint.fix_body(body), body)

    def test_leading_paren_ref_is_left_for_hand_rewrite(self):
        # No preceding text on the line -- same caveat as
        # code-coordinator's comment_cruft.py: at the start of a line a
        # parenthetical can't be told apart from load-bearing content.
        body = "(#1005) keep this guard"
        self.assertEqual(lint.fix_body(body), body)

    def test_trailing_paren_ref_after_text_is_stripped(self):
        self.assertEqual(lint.fix_body("keep this guard (#1005)"), "keep this guard")

    def test_history_prose_is_left_unchanged(self):
        body = "this used to return early before #99 landed"
        self.assertEqual(lint.fix_body(body), body)


class FixFileTest(unittest.TestCase):
    def _write(self, tmp: str, text: str) -> Path:
        path = Path(tmp) / "sample.rs"
        path.write_text(text)
        return path

    def test_own_line_leading_tag_comment_is_rewritten_in_place(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, "fn f() {\n    // #1630 review: tightened the bound check\n    1\n}\n")
            changed, remaining = lint.fix_file(path, {2}, docs=False)
            self.assertTrue(changed)
            self.assertEqual(remaining, [])
            self.assertIn("// Tightened the bound check\n", path.read_text())

    def test_ref_only_own_line_comment_is_deleted(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, "fn f() {\n    // see #883\n    1\n}\n")
            changed, remaining = lint.fix_file(path, {2}, docs=False)
            self.assertTrue(changed)
            self.assertEqual(remaining, [])
            text = path.read_text()
            self.assertNotIn("#883", text)
            self.assertEqual(text.count("\n"), 3)

    def test_trailing_comment_on_code_keeps_the_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, "let x = 1; // see #883\n")
            changed, remaining = lint.fix_file(path, {1}, docs=False)
            self.assertTrue(changed)
            self.assertEqual(remaining, [])
            self.assertEqual(path.read_text(), "let x = 1;\n")

    def test_history_prose_is_reported_as_remaining_and_untouched(self):
        with tempfile.TemporaryDirectory() as tmp:
            text = "fn f() {\n    // this used to return early before #99 landed\n    1\n}\n"
            path = self._write(tmp, text)
            changed, remaining = lint.fix_file(path, {2}, docs=False)
            self.assertFalse(changed)
            self.assertEqual([r[0] for r in remaining], [2])
            self.assertEqual(path.read_text(), text)

    def test_doc_comment_is_left_alone_without_docs_flag(self):
        with tempfile.TemporaryDirectory() as tmp:
            text = "/// workaround for #4321\nfn f() {}\n"
            path = self._write(tmp, text)
            changed, remaining = lint.fix_file(path, {1}, docs=False)
            self.assertFalse(changed)
            self.assertEqual([r[0] for r in remaining], [1])
            self.assertEqual(path.read_text(), text)

    def test_doc_comment_is_fixed_with_docs_flag(self):
        with tempfile.TemporaryDirectory() as tmp:
            text = "/// #1630 review: tightened the bound check\nfn f() {}\n"
            path = self._write(tmp, text)
            changed, remaining = lint.fix_file(path, {1}, docs=True)
            self.assertTrue(changed)
            self.assertEqual(remaining, [])
            self.assertIn("/// Tightened the bound check\n", path.read_text())


class MainExitStatusTest(unittest.TestCase):
    def test_report_only_always_exits_zero(self):
        with tempfile.TemporaryDirectory() as tmp:
            thresholds = Path(tmp) / "thresholds.json"
            thresholds.write_text("{}")
            rc = lint.main(["--report-only", "--thresholds", str(thresholds)])
            self.assertEqual(rc, 0)

    def test_missing_thresholds_file_is_treated_as_unthresholded(self):
        with tempfile.TemporaryDirectory() as tmp:
            missing = Path(tmp) / "does_not_exist.json"
            rc = lint.main(["--thresholds", str(missing)])
            self.assertEqual(rc, 0)

    def test_group_over_its_threshold_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            thresholds = Path(tmp) / "thresholds.json"
            # Every real group has far more than 0 flagged lines today, so a
            # 0 threshold on every gated group guarantees at least one is
            # over -- this exercises the gate without hardcoding today's
            # repo-wide counts into a test (which would churn every time the
            # baseline is ratcheted down by a module pass).
            thresholds.write_text(
                '{"src": 0, "src/app": 0, "src/render": 0, '
                '"src/core/engine": 0, "src/core": 0, "tests": 0, "backends": 0}'
            )
            rc = lint.main(["--thresholds", str(thresholds)])
            self.assertEqual(rc, 1)

    def test_default_thresholds_file_passes_on_this_checkout(self):
        # Acceptance check for #1896: the shipped thresholds file must pass
        # against this repo as it stands right now.
        rc = lint.main(["--report-only"])
        self.assertEqual(rc, 0)
        rc = lint.main([])
        self.assertEqual(rc, 0)

    def test_fix_with_explicit_path_cleans_a_scratch_file_under_the_real_tree(self):
        # --fix walks the real SCAN_ROOTS (iter_scan_files isn't
        # parameterized the way group_for is), so this plants a throwaway
        # file under the real repo's src/ rather than a fake root, and
        # removes it again whether or not the assertions pass.
        scratch = lint.REPO_ROOT / "src" / "zz_fix_test_scratch.rs"
        scratch.write_text("fn f() {\n    // #1630 review: tightened the bound check\n    1\n}\n")
        try:
            rc = lint.main(["--fix", "--report-only", str(scratch)])
            self.assertEqual(rc, 0)
            self.assertIn("// Tightened the bound check\n", scratch.read_text())
        finally:
            scratch.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
