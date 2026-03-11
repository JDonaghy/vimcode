mod common;
use common::*;
use vimcode_core::Mode;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn engine_with_md(md_content: &str) -> vimcode_core::Engine {
    let path = std::env::temp_dir().join("vimcode_test_preview.md");
    std::fs::write(&path, md_content).unwrap();
    let mut e = engine_with("");
    e.open_file_in_tab(&path);
    e
}

// ─── :MarkdownPreview on .md file ────────────────────────────────────────────

#[test]
fn markdown_preview_opens_split() {
    let mut e = engine_with_md("# Hello\n\nSome text");
    let before_windows = e.windows.len();
    exec(&mut e, "MarkdownPreview");
    // A new window should be created (vsplit).
    assert!(e.windows.len() > before_windows);
    assert!(e.message.contains("[Preview]"));
}

#[test]
fn markdown_preview_on_non_md_errors() {
    let path = std::env::temp_dir().join("vimcode_test_not_md.txt");
    std::fs::write(&path, "hello").unwrap();
    let mut e = engine_with("");
    e.open_file_in_tab(&path);
    let action = exec(&mut e, "MarkdownPreview");
    assert_eq!(action, vimcode_core::EngineAction::Error);
    assert!(e.message.contains("Not a markdown file"));
}

#[test]
fn md_preview_alias_works() {
    let mut e = engine_with_md("# Test");
    exec(&mut e, "MdPreview");
    assert!(e.message.contains("[Preview]"));
}

// ─── Read-only guard ─────────────────────────────────────────────────────────

#[test]
fn read_only_blocks_insert_mode() {
    let mut e = engine_with_md("# Title");
    exec(&mut e, "MarkdownPreview");
    // Active window is now the preview buffer (read-only).
    // Try insert-mode keys — they should be blocked.
    for ch in ['i', 'a', 'o', 'O', 'I', 'A', 's', 'S', 'c', 'C', 'R'] {
        press(&mut e, ch);
        assert_eq!(
            e.mode,
            Mode::Normal,
            "key '{ch}' should not enter insert mode in read-only"
        );
        assert!(
            e.message.contains("read-only"),
            "key '{ch}' should show read-only message, got: {:?}",
            e.message
        );
    }
}

// ─── Static preview (open_markdown_preview) ──────────────────────────────────

#[test]
fn open_markdown_preview_creates_read_only_buffer() {
    let mut e = engine_with("some text");
    let preview_id = e.open_markdown_preview("# Hello World\n\nContent here", "Test Preview");
    let state = e.buffer_manager.get(preview_id).unwrap();
    assert!(state.read_only);
    assert!(state.md_rendered.is_some());
    let md = state.md_rendered.as_ref().unwrap();
    assert!(!md.lines.is_empty());
    // The rendered text should contain "Hello World" (without the '#').
    assert!(md.lines[0].contains("Hello World"));
}

#[test]
fn preview_buffer_has_rendered_content() {
    let mut e = engine_with("");
    let buf_id = e.open_markdown_preview("**bold** and `code`", "test");
    let state = e.buffer_manager.get(buf_id).unwrap();
    let md = state.md_rendered.as_ref().unwrap();
    // Should have text with bold and code (markdown syntax stripped).
    let first = &md.lines[0];
    assert!(first.contains("bold"), "expected 'bold' in: {first}");
    assert!(first.contains("code"), "expected 'code' in: {first}");
}

// ─── Live preview link ───────────────────────────────────────────────────────

#[test]
fn live_preview_tracked_in_md_preview_links() {
    let mut e = engine_with_md("# Source");
    exec(&mut e, "MarkdownPreview");
    // Should have at least one entry in md_preview_links.
    assert!(
        !e.md_preview_links.is_empty(),
        "md_preview_links should track the preview"
    );
}

#[test]
fn live_preview_refreshes_on_source_edit() {
    let mut e = engine_with_md("# Original");
    // Get source buffer id.
    let source_id = e.active_buffer_id();
    exec(&mut e, "MarkdownPreview");
    // Preview is now active. Get preview buf id.
    let preview_id = e.active_buffer_id();
    assert_ne!(source_id, preview_id);

    // Find the window showing the source buffer and switch to it.
    let source_wid = e
        .windows
        .iter()
        .find(|(_, w)| w.buffer_id == source_id)
        .map(|(&id, _)| id)
        .expect("source window should exist");
    let tab = e.active_tab_mut();
    tab.active_window = source_wid;

    assert_eq!(e.active_buffer_id(), source_id);

    // Edit the source.
    press(&mut e, 'A'); // append at end of line
    type_chars(&mut e, " Updated");
    press_key(&mut e, "Escape");

    // The preview buffer content should have been refreshed.
    let preview_state = e.buffer_manager.get(preview_id).unwrap();
    let preview_text = preview_state.buffer.to_string();
    assert!(
        preview_text.contains("Updated"),
        "preview should reflect source edit, got: {preview_text}"
    );
}

// ─── Link cleanup on close ───────────────────────────────────────────────────

#[test]
fn preview_link_cleaned_on_close() {
    let mut e = engine_with_md("# Test");
    exec(&mut e, "MarkdownPreview");
    assert!(!e.md_preview_links.is_empty());

    // The active window is the preview (in a split).  Closing it should remove
    // the preview buffer and clean up the link.
    // close_window removes the window but buffer GC only happens in close_tab.
    // Use open_markdown_preview directly and track the buffer ID.
    let preview_id = e.active_buffer_id();
    assert!(e.buffer_manager.get(preview_id).unwrap().read_only);

    // Close the preview window. The buffer stays alive (close_window doesn't GC),
    // but verify the link entry exists.
    e.close_window();

    // Now open a second tab + preview to test close_tab cleanup.
    // We need another tab so that close_tab actually succeeds.
    exec(&mut e, "tabnew");
    // Create a preview in this tab using open_markdown_preview (not linked).
    let buf_id = e.open_markdown_preview("# Temp", "temp");
    e.md_preview_links.insert(buf_id, e.active_buffer_id());

    // Now there should be a link.
    assert!(e.md_preview_links.contains_key(&buf_id));
    e.close_tab();
    // After close_tab GC, the link should be removed.
    assert!(
        !e.md_preview_links.contains_key(&buf_id),
        "preview links should be cleaned up after close_tab"
    );
}

// ─── Extension README via sidebar ────────────────────────────────────────────

#[test]
fn extension_readme_opens_in_own_tab() {
    let mut e = engine_with("");
    // Simulate having an installed extension with a README on disk.
    e.extension_state.installed.push("rust".to_string());
    // Seed the registry so ext_installed_items() can find the manifest.
    e.ext_registry = Some(vec![vimcode_core::core::extensions::ExtensionManifest {
        name: "rust".to_string(),
        display_name: "Rust".to_string(),
        ..Default::default()
    }]);

    // Create a temporary README file so the sidebar can read it
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let ext_dir = std::path::PathBuf::from(&home).join(".config/vimcode/extensions/rust");
    let _ = std::fs::create_dir_all(&ext_dir);
    let readme_path = ext_dir.join("README.md");
    std::fs::write(&readme_path, "# Rust Extension\nREADME content").unwrap();

    e.ext_sidebar_has_focus = true;
    e.ext_sidebar_sections_expanded = [true, false];
    e.ext_sidebar_selected = 0; // First installed item.

    let before_tabs = e.active_group().tabs.len();
    e.handle_ext_sidebar_key("Return", false, None);
    // Should have opened in a new tab (not a vsplit).
    assert_eq!(
        e.active_group().tabs.len(),
        before_tabs + 1,
        "README should open in a new tab"
    );
    // The new buffer should be read-only with md_rendered.
    let buf_id = e.active_buffer_id();
    let state = e.buffer_manager.get(buf_id).unwrap();
    assert!(state.read_only);
    assert!(state.md_rendered.is_some());

    // Clean up
    let _ = std::fs::remove_file(&readme_path);
}

// ─── Undo/redo refreshes preview ────────────────────────────────────────────

#[test]
fn undo_refreshes_linked_preview() {
    let mut e = engine_with_md("# Original");
    let source_id = e.active_buffer_id();
    exec(&mut e, "MarkdownPreview");
    let preview_id = e.active_buffer_id();
    assert_ne!(source_id, preview_id);

    // Switch back to source window.
    let source_wid = e
        .windows
        .iter()
        .find(|(_, w)| w.buffer_id == source_id)
        .map(|(&id, _)| id)
        .unwrap();
    let tab = e.active_tab_mut();
    tab.active_window = source_wid;

    // Edit the source.
    press(&mut e, 'A');
    type_chars(&mut e, " Changed");
    press_key(&mut e, "Escape");

    // Verify preview has "Changed".
    let preview_text = e.buffer_manager.get(preview_id).unwrap().buffer.to_string();
    assert!(
        preview_text.contains("Changed"),
        "preview should reflect edit"
    );

    // Undo.
    press(&mut e, 'u');

    // Preview should revert to "Original" (no "Changed").
    let preview_text = e.buffer_manager.get(preview_id).unwrap().buffer.to_string();
    assert!(
        !preview_text.contains("Changed"),
        "undo should refresh preview, got: {preview_text}"
    );
}

#[test]
fn redo_refreshes_linked_preview() {
    let mut e = engine_with_md("# Start");
    let source_id = e.active_buffer_id();
    exec(&mut e, "MarkdownPreview");
    let preview_id = e.active_buffer_id();

    // Switch back to source.
    let source_wid = e
        .windows
        .iter()
        .find(|(_, w)| w.buffer_id == source_id)
        .map(|(&id, _)| id)
        .unwrap();
    e.active_tab_mut().active_window = source_wid;

    // Edit, then undo.
    press(&mut e, 'A');
    type_chars(&mut e, " Added");
    press_key(&mut e, "Escape");
    press(&mut e, 'u');

    let preview_text = e.buffer_manager.get(preview_id).unwrap().buffer.to_string();
    assert!(!preview_text.contains("Added"));

    // Redo (Ctrl-R).
    ctrl(&mut e, 'r');

    let preview_text = e.buffer_manager.get(preview_id).unwrap().buffer.to_string();
    assert!(
        preview_text.contains("Added"),
        "redo should refresh preview, got: {preview_text}"
    );
}

// ─── Scroll sync ────────────────────────────────────────────────────────────

#[test]
fn markdown_preview_registers_scroll_bind() {
    let mut e = engine_with_md("# Title\n\nLine 1\nLine 2\nLine 3");
    let before_binds = e.scroll_bind_pairs.len();
    exec(&mut e, "MarkdownPreview");
    assert_eq!(
        e.scroll_bind_pairs.len(),
        before_binds + 1,
        "should register a scroll bind pair"
    );
}

// ─── Markdown parser unit tests are in src/core/markdown.rs ──────────────────
// The 15 unit tests there cover: headings, bold, italic, code, lists, links, etc.

#[test]
fn markdown_renderer_produces_styled_spans() {
    // Quick integration-level check that MdRendered works end-to-end.
    use vimcode_core::core::markdown::{render_markdown, MdStyle};
    let r = render_markdown("# Title\n\n**bold text** and *italic*");
    assert!(!r.lines.is_empty());
    // Title line should have heading spans.
    assert!(
        r.spans[0].iter().any(|s| s.style == MdStyle::Heading(1)),
        "expected H1 span"
    );
    // Find the line with bold+italic.
    let bold_line = r
        .lines
        .iter()
        .position(|l| l.contains("bold text"))
        .unwrap();
    assert!(
        r.spans[bold_line].iter().any(|s| s.style == MdStyle::Bold),
        "expected Bold span"
    );
    assert!(
        r.spans[bold_line]
            .iter()
            .any(|s| s.style == MdStyle::Italic),
        "expected Italic span"
    );
}
