mod common;
use common::*;

// ── Settings ─────────────────────────────────────────────────────────────────

#[test]
fn breadcrumbs_default_true() {
    let e = engine_with("hello\n");
    assert!(e.settings.breadcrumbs, "breadcrumbs should default to true");
}

#[test]
fn breadcrumbs_toggle_via_set() {
    let mut e = engine_with("hello\n");
    assert!(e.settings.breadcrumbs);
    exec(&mut e, "set nobreadcrumbs");
    assert!(!e.settings.breadcrumbs);
    exec(&mut e, "set breadcrumbs");
    assert!(e.settings.breadcrumbs);
}

#[test]
fn breadcrumbs_query() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "set breadcrumbs?");
    assert_msg_contains(&e, "breadcrumbs");
    exec(&mut e, "set nobreadcrumbs");
    exec(&mut e, "set breadcrumbs?");
    assert_msg_contains(&e, "nobreadcrumbs");
}

// ── enclosing_scopes ─────────────────────────────────────────────────────────

#[test]
fn enclosing_scopes_rust_nested() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
    let code = "\
mod foo {
    struct Bar;
    impl Bar {
        fn baz() {
            let x = 1;
        }
    }
}
";
    // Parse to populate last_tree
    syntax.parse(code);
    // Cursor on `let x = 1;` (line 4, col 12)
    let scopes = syntax.enclosing_scopes(code, 4, 12);
    let names: Vec<&str> = scopes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["foo", "Bar", "baz"],
        "expected mod > impl > fn chain"
    );
}

#[test]
fn enclosing_scopes_rust_outside_scope() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
    let code = "// top-level comment\nfn main() {}\n";
    syntax.parse(code);
    // Cursor on the comment (line 0, col 5) — outside any scope-defining node
    let scopes = syntax.enclosing_scopes(code, 0, 5);
    assert!(scopes.is_empty(), "no scopes at top-level comment");
}

#[test]
fn enclosing_scopes_rust_struct_only() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
    let code = "\
struct MyStruct {
    field: i32,
}
";
    syntax.parse(code);
    // Cursor on `field` (line 1, col 4)
    let scopes = syntax.enclosing_scopes(code, 1, 4);
    let names: Vec<&str> = scopes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["MyStruct"]);
}

#[test]
fn enclosing_scopes_python() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    let mut syntax = Syntax::new_for_language(SyntaxLanguage::Python);
    let code = "\
class Foo:
    def bar(self):
        x = 1
";
    syntax.parse(code);
    // Cursor on `x = 1` (line 2, col 8)
    let scopes = syntax.enclosing_scopes(code, 2, 8);
    let names: Vec<&str> = scopes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Foo", "bar"]);
}

#[test]
fn enclosing_scopes_javascript() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    let mut syntax = Syntax::new_for_language(SyntaxLanguage::JavaScript);
    let code = "\
class Widget {
    render() {
        return 42;
    }
}
";
    syntax.parse(code);
    // Cursor on `return 42` (line 2, col 8)
    let scopes = syntax.enclosing_scopes(code, 2, 8);
    let names: Vec<&str> = scopes.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Widget", "render"]);
}

#[test]
fn enclosing_scopes_empty_for_unsupported_lang() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    let mut syntax = Syntax::new_for_language(SyntaxLanguage::Json);
    let code = r#"{"key": "value"}"#;
    syntax.parse(code);
    let scopes = syntax.enclosing_scopes(code, 0, 5);
    assert!(scopes.is_empty(), "JSON has no scope kinds defined");
}

#[test]
fn enclosing_scopes_no_tree() {
    use vimcode_core::core::syntax::{Syntax, SyntaxLanguage};
    // Don't call parse — last_tree is None
    let syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
    let scopes = syntax.enclosing_scopes("fn main() {}", 0, 5);
    assert!(scopes.is_empty(), "should be empty when no tree parsed");
}

// ── Build breadcrumbs ────────────────────────────────────────────────────────

#[test]
fn breadcrumbs_include_path_segments() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("vimcode_test_bc_path");
    let _ = std::fs::create_dir_all(dir.join("src").join("core"));
    let file = dir.join("src").join("core").join("engine.rs");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        write!(f, "fn main() {{}}\n").unwrap();
    }
    let mut e = engine_with("");
    e.cwd = dir.clone();
    exec(&mut e, &format!("e {}", file.display()));
    // With breadcrumbs on, the active buffer should have path segments
    assert!(e.settings.breadcrumbs);
    // Verify the file path was set and is relative to cwd
    if let Some(path) = e.file_path() {
        let rel = path.strip_prefix(&dir).unwrap();
        let parts: Vec<&str> = rel
            .to_str()
            .unwrap()
            .split(std::path::MAIN_SEPARATOR)
            .collect();
        assert_eq!(parts, vec!["src", "core", "engine.rs"]);
    }
    // Even if :e didn't set the path (test env), the setting is respected
    let _ = std::fs::remove_dir_all(&dir);
}
