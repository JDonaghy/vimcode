use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use ropey::Rope;

/// Unique identifier for a buffer within the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);

impl fmt::Display for BufferId {
    /// Renders as the bare numeric id — used as the opaque document id
    /// handed to quadraui's `WorkspaceController` preview tier (#658).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Buffer {
    #[allow(dead_code)]
    pub id: BufferId,
    pub content: Rope,
}

impl Buffer {
    pub fn new(id: BufferId) -> Self {
        Self {
            id,
            content: Rope::new(),
        }
    }

    #[allow(dead_code)]
    pub fn from_text(id: BufferId, text: &str) -> Self {
        Self {
            id,
            content: Rope::from_str(text),
        }
    }

    /// Load buffer contents from a file. Returns an io::Error if reading fails.
    ///
    /// Reads raw bytes rather than `fs::read_to_string` and sniffs a BOM
    /// first (#1560): Windows text editors (Notepad, PowerShell `Out-File`,
    /// etc.) routinely write UTF-16LE-with-BOM, and some tools write
    /// UTF-8-with-BOM. A bare `read_to_string` rejects both with an opaque
    /// "stream did not contain valid UTF-8" and — because the error bubbles
    /// out of `open_file`/`reopen_buffer` before any buffer is created or
    /// replaced — leaves whatever buffer was already active on screen,
    /// which can look like an unrelated `[No Name]` tab silently "holding"
    /// the file's content when it's really just the untouched buffer that
    /// was current before the failed open.
    pub fn from_file(id: BufferId, path: &Path) -> Result<Self, io::Error> {
        let text = read_file_to_string(path)?;
        Ok(Self {
            id,
            content: Rope::from_str(&text),
        })
    }

    /// Write buffer contents to a file.
    pub fn save_to_file(&self, path: &Path) -> Result<(), io::Error> {
        fs::write(path, self.to_string())
    }

    pub fn insert(&mut self, char_idx: usize, text: &str) {
        if char_idx <= self.content.len_chars() {
            self.content.insert(char_idx, text);
        }
    }

    pub fn delete_range(&mut self, start_idx: usize, end_idx: usize) {
        if start_idx < end_idx && end_idx <= self.content.len_chars() {
            self.content.remove(start_idx..end_idx);
        }
    }

    #[allow(dead_code)]
    pub fn len_chars(&self) -> usize {
        self.content.len_chars()
    }

    pub fn line_to_char(&self, line_idx: usize) -> usize {
        self.content.line_to_char(line_idx)
    }

    /// Returns the number of visible lines in the buffer.
    ///
    /// Ropey's `len_lines()` counts a trailing `\n` as starting a new (empty)
    /// line. For cursor navigation we want the count of lines that actually
    /// contain content, so we subtract 1 when the text ends with `\n`.
    pub fn len_lines(&self) -> usize {
        let n = self.content.len_lines();
        if n > 1
            && self.content.len_chars() > 0
            && self.content.char(self.content.len_chars() - 1) == '\n'
        {
            n - 1
        } else {
            n
        }
    }

    pub fn line_len_chars(&self, line_idx: usize) -> usize {
        if line_idx >= self.len_lines() {
            return 0;
        }
        self.content.line(line_idx).len_chars()
    }
}

/// Read a whole file from `path` and decode it to a `String`, transcoding
/// the BOM'd encodings that Windows text tools commonly emit (#1560). Shared
/// by `Buffer::from_file` and `BufferState::reload_from_disk` (`:e!` and the
/// idle file-watcher's silent-reload path) so every read of a file's
/// contents — initial open or later reload — goes through the same
/// BOM-aware decode instead of a strict `fs::read_to_string` that rejects
/// non-UTF-8 bytes outright.
pub(crate) fn read_file_to_string(path: &Path) -> Result<String, io::Error> {
    let bytes = fs::read(path)?;
    decode_file_bytes(&bytes)
}

/// Decode a whole file's bytes to a `String`, transcoding the BOM'd
/// encodings that Windows text tools commonly emit (#1560):
///
/// - UTF-8 with BOM (`EF BB BF`) — strip the BOM, decode the rest as UTF-8.
/// - UTF-16LE with BOM (`FF FE`) — Notepad's and PowerShell `Out-File`'s
///   default when writing "Unicode" text.
/// - UTF-16BE with BOM (`FE FF`) — rarer, but the mirror image is trivial
///   once UTF-16LE is handled.
///
/// UTF-32LE (`FF FE 00 00`) and UTF-32BE (`00 00 FE FF`) BOMs are checked
/// *before* UTF-16, since the UTF-16LE BOM is a byte-for-byte prefix of the
/// UTF-32LE one — without this a UTF-32LE file would be misdetected as
/// UTF-16LE and decoded into garbage instead of surfacing a clear error.
/// UTF-32 itself isn't decoded (rare on Windows, and `char::decode_utf16`
/// doesn't help here); it surfaces the same `io::Error` as any other
/// unsupported encoding.
///
/// With no recognized BOM, falls back to strict UTF-8 (matching the old
/// `fs::read_to_string` behavior) so a genuinely non-UTF-8 file still
/// surfaces a clear `io::Error` instead of silently mangling bytes.
fn decode_file_bytes(bytes: &[u8]) -> Result<String, io::Error> {
    const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
    const UTF32LE_BOM: [u8; 4] = [0xFF, 0xFE, 0x00, 0x00];
    const UTF32BE_BOM: [u8; 4] = [0x00, 0x00, 0xFE, 0xFF];
    const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];
    const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];

    if bytes.starts_with(&UTF8_BOM) {
        return std::str::from_utf8(&bytes[UTF8_BOM.len()..])
            .map(str::to_string)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    }
    if bytes.starts_with(&UTF32LE_BOM) || bytes.starts_with(&UTF32BE_BOM) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UTF-32 encoded files are not supported",
        ));
    }
    if bytes.starts_with(&UTF16LE_BOM) {
        return decode_utf16_bytes(&bytes[UTF16LE_BOM.len()..], u16::from_le_bytes);
    }
    if bytes.starts_with(&UTF16BE_BOM) {
        return decode_utf16_bytes(&bytes[UTF16BE_BOM.len()..], u16::from_be_bytes);
    }
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Decode a UTF-16 byte stream (post-BOM) into a `String`, using
/// `from_pair` to pick LE vs BE byte order for each 16-bit code unit.
fn decode_utf16_bytes(bytes: &[u8], from_pair: fn([u8; 2]) -> u16) -> Result<String, io::Error> {
    if !bytes.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UTF-16 file has a trailing byte with no pair after its BOM",
        ));
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|pair| from_pair(*pair));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

impl fmt::Display for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.content)
    }
}

// ─── Decorations: namespaces, extmarks, highlights, virtual text, signs ────
//
// #1653 (Native API P5). Lives alongside `Buffer`/`BufferId` rather than in
// its own module because every decoration is anchored to a `BufferId` and
// shifted by the same line-insert/line-delete events `Buffer` already models
// — see `Engine::shift_marks_for_line_insert`/`shift_marks_for_line_delete`
// in `engine/buffers.rs`, which call the `DecorState` methods below right
// alongside the pre-existing vim-mark (`Engine::marks`) shift.
//
// Deliberately NOT a reimplementation of `Engine::line_annotations` /
// `annotate_line` — those keep their existing, unchanged behaviour (frozen
// by `tests/extensions.rs`). This is the new, richer mechanism
// `vimcode.decor.*` exposes: namespaced, range-capable, tracked through
// undo/redo, with named highlight groups and gutter signs.
//
// Colour/theme resolution deliberately does NOT live here: `core/` has no
// `Theme` (that's `render.rs`, outside `core/` by design), so `HlGroupDef`
// stores the raw spec a plugin registered (hex strings, bool flags, an
// optional `link` name) and `render.rs` resolves it against the active
// `Theme` when building paint data — the same split `Theme` itself already
// has from `core::settings`.

/// Opaque id for a `vimcode.decor.namespace()` namespace. Every decoration
/// belongs to exactly one; `DecorState::clear` removes only marks in the
/// given namespace, so two plugins each holding their own `NamespaceId`
/// cannot see or clear each other's marks even though they share one
/// `DecorState` (per-buffer, not per-plugin).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NamespaceId(pub u32);

/// Opaque id for one `vimcode.decor.set_mark()` extmark. Unique within the
/// `DecorState` that minted it (i.e. engine-wide, not just within one
/// buffer) so a stale id from a different buffer can never collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MarkId(pub u64);

/// Where a mark's virtual text paints relative to its anchor column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtTextPos {
    /// After the end of the line's real content, in a muted colour — the
    /// same visual slot `annotate_line` uses (though implemented
    /// independently; see the module doc above).
    Eol,
    /// Replaces the glyph(s) already at the mark's column, same width —
    /// does not shift later text. Used for jump-label-style overlays.
    Overlay,
    /// Inserted at the mark's column, shifting later text (and the cursor,
    /// when it sits past the insertion point on the same line) right.
    Inline,
}

/// One `{text, hl_group}` chunk of virtual text (`vimcode.decor.set_mark`'s
/// `virt_text` option is a list of these, like Neovim's extmark API).
#[derive(Debug, Clone, PartialEq)]
pub struct VirtTextChunk {
    pub text: String,
    pub hl_group: Option<String>,
}

/// Per-mark decoration payload (everything but its anchor position).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecorOpts {
    /// Highlights the mark's `[row,col)..(end_row,end_col)` range.
    pub hl_group: Option<String>,
    pub virt_text: Vec<VirtTextChunk>,
    /// Required (consulted) only when `virt_text` is non-empty.
    pub virt_text_pos: Option<VirtTextPos>,
    /// 1-2 display cells painted in the gutter's sign column.
    pub sign_text: Option<String>,
    pub sign_hl: Option<String>,
}

/// One namespaced extmark: a point or range anchored to 0-indexed `(row,
/// col)` buffer coordinates, kept in sync with edits by
/// [`BufferDecorations::shift_insert`]/[`shift_delete`]/
/// [`shift_for_text_replace`]. A point mark has `end_row == row && end_col ==
/// col`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecorMark {
    pub id: MarkId,
    pub ns: NamespaceId,
    pub row: usize,
    pub col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub opts: DecorOpts,
}

/// A registered `vimcode.decor.set_hl()` highlight group. Colour fields are
/// the raw `"#rrggbb"`/`"#rrggbbaa"` strings a plugin passed in (or a plain
/// named token — `render.rs` resolves those too); `None` leaves that
/// channel untouched (e.g. `fg` set, `bg` absent, inherits the editor
/// background). `link` names another group whose *resolved* colours/flags
/// are used instead — at most one hop is honoured (mirrors Neovim: linking
/// to a group that itself links is not chased further), so it keeps
/// following theme switches (`ColorScheme`) rather than freezing the
/// linked-to colour at `set_hl` time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HlGroupDef {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub link: Option<String>,
}

/// Every decoration mark for one buffer, indexed by starting row for
/// O(visible-range) lookups — [`marks_touching`](Self::marks_touching) never
/// scans marks anchored off-screen, satisfying #1653's "off-screen lines
/// must cost nothing per frame" requirement without a per-frame full scan.
#[derive(Debug, Clone, Default)]
pub struct BufferDecorations {
    marks: HashMap<MarkId, DecorMark>,
    /// Starting row -> mark ids anchored there. Every entry here names a
    /// live key in `marks`; kept in sync by every mutator below.
    by_row: BTreeMap<usize, Vec<MarkId>>,
}

impl BufferDecorations {
    fn index_insert(&mut self, row: usize, id: MarkId) {
        self.by_row.entry(row).or_default().push(id);
    }

    fn index_remove(&mut self, row: usize, id: MarkId) {
        if let Some(ids) = self.by_row.get_mut(&row) {
            ids.retain(|&m| m != id);
            if ids.is_empty() {
                self.by_row.remove(&row);
            }
        }
    }

    pub fn insert(&mut self, mark: DecorMark) {
        self.index_insert(mark.row, mark.id);
        self.marks.insert(mark.id, mark);
    }

    pub fn get(&self, id: MarkId) -> Option<&DecorMark> {
        self.marks.get(&id)
    }

    pub fn remove(&mut self, id: MarkId) -> Option<DecorMark> {
        let mark = self.marks.remove(&id)?;
        self.index_remove(mark.row, id);
        Some(mark)
    }

    /// Marks whose `[row, end_row]` span touches the visible `[start_row,
    /// end_row]` window — i.e. every mark a renderer needs to paint this
    /// frame. Only walks `by_row` entries up to `end_row`, so a mark anchored
    /// past the visible range (however many thousands of lines below) is
    /// never visited.
    pub fn marks_touching(&self, start_row: usize, end_row: usize) -> Vec<&DecorMark> {
        self.by_row
            .range(..=end_row)
            .flat_map(|(_, ids)| ids.iter())
            .filter_map(|id| self.marks.get(id))
            .filter(|m| m.end_row >= start_row)
            .collect()
    }

    /// Remove every mark in `ns`, optionally restricted to marks whose range
    /// touches 0-indexed row range `[start, end)`.
    pub fn clear_ns(&mut self, ns: NamespaceId, range: Option<(usize, usize)>) {
        let to_remove: Vec<MarkId> = self
            .marks
            .values()
            .filter(|m| m.ns == ns && range.is_none_or(|(s, e)| m.row < e && m.end_row >= s))
            .map(|m| m.id)
            .collect();
        for id in to_remove {
            self.remove(id);
        }
    }

    /// Apply `f` to every mark's `(row, end_row)`, relocating it in `by_row`
    /// when its starting row changes. `f` returning `None` removes the mark
    /// (a range wholly consumed by a deletion collapses rather than dangling
    /// on whatever line slid into its old slot — same rule real Vim's plain
    /// marks follow, see `Engine::shift_marks_for_line_delete`).
    fn relocate(&mut self, mut f: impl FnMut(&DecorMark) -> Option<(usize, usize)>) {
        let mut removals = Vec::new();
        let mut moves: Vec<(MarkId, usize, usize, usize)> = Vec::new();
        for m in self.marks.values() {
            match f(m) {
                None => removals.push(m.id),
                Some((row, end_row)) => {
                    if row != m.row || end_row != m.end_row {
                        moves.push((m.id, m.row, row, end_row));
                    }
                }
            }
        }
        for id in removals {
            self.remove(id);
        }
        for (id, old_row, new_row, new_end_row) in moves {
            self.index_remove(old_row, id);
            if let Some(m) = self.marks.get_mut(&id) {
                m.row = new_row;
                m.end_row = new_end_row;
            }
            self.index_insert(new_row, id);
        }
    }

    /// Shift every mark the way Vim shifts its own marks for a full-line
    /// insertion (mirrors `Engine::shift_marks_for_line_insert`): rows
    /// strictly below `at_line` move down by `line_count`; when the
    /// insertion also pushes `at_line`'s own original content down
    /// (`at_line_start` — `O`, `:put` above, …), a mark sitting exactly on
    /// `at_line` moves with it instead of being left pointing at the new,
    /// blank line.
    pub fn shift_insert(&mut self, at_line: usize, line_count: usize, at_line_start: bool) {
        if line_count == 0 {
            return;
        }
        let shifts = |line: usize| line > at_line || (at_line_start && line == at_line);
        self.relocate(|m| {
            let row = if shifts(m.row) {
                m.row + line_count
            } else {
                m.row
            };
            let end_row = if shifts(m.end_row) {
                m.end_row + line_count
            } else {
                m.end_row
            };
            Some((row, end_row))
        });
    }

    /// Shift/collapse marks for a full-line deletion covering `[at_line,
    /// at_line + line_count)` (mirrors `Engine::shift_marks_for_line_delete`).
    /// A mark entirely inside the removed range collapses; one that merely
    /// straddles the boundary clamps the affected end to `at_line` instead —
    /// part of its range survives, so unlike a fully-contained mark it isn't
    /// removed outright.
    pub fn shift_delete(&mut self, at_line: usize, line_count: usize) {
        if line_count == 0 {
            return;
        }
        let end = at_line + line_count;
        self.relocate(|m| {
            let row_inside = m.row >= at_line && m.row < end;
            let end_row_inside = m.end_row >= at_line && m.end_row < end;
            if row_inside && end_row_inside {
                return None;
            }
            let row = if m.row >= end {
                m.row - line_count
            } else if row_inside {
                at_line
            } else {
                m.row
            };
            let end_row = if m.end_row >= end {
                m.end_row - line_count
            } else if end_row_inside {
                at_line
            } else {
                m.end_row
            };
            Some((row, end_row))
        });
    }

    /// Relocate every mark across an undo/redo (or `g-`/`g+`/`:earlier`/
    /// `:later`) jump, which replaces the buffer's *entire* text in one step
    /// with no per-edit line/count info to shift by. Finds the common prefix
    /// and suffix lines between `old_text` and `new_text` (split on `'\n'`,
    /// the same convention `char_to_line`-derived rows use): marks strictly
    /// inside the common prefix are untouched, marks strictly inside the
    /// common suffix shift by the old/new line-count delta, and any mark
    /// touching the changed middle section collapses — its surrounding text
    /// no longer reads the same, so there's no sound position left to claim
    /// (same "whole range gone" collapse rule as `shift_delete`).
    pub fn shift_for_text_replace(&mut self, old_text: &str, new_text: &str) {
        if old_text == new_text {
            return;
        }
        let old_lines: Vec<&str> = old_text.split('\n').collect();
        let new_lines: Vec<&str> = new_text.split('\n').collect();
        let max_common = old_lines.len().min(new_lines.len());
        let mut prefix = 0;
        while prefix < max_common && old_lines[prefix] == new_lines[prefix] {
            prefix += 1;
        }
        let max_suffix = max_common - prefix;
        let mut suffix = 0;
        while suffix < max_suffix
            && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
        {
            suffix += 1;
        }
        let old_change_end = old_lines.len() - suffix;
        let delta = new_lines.len() as isize - old_lines.len() as isize;
        self.relocate(|m| {
            if m.row < prefix && m.end_row < prefix {
                return Some((m.row, m.end_row));
            }
            if m.row >= old_change_end && m.end_row >= old_change_end {
                let row = (m.row as isize + delta).max(0) as usize;
                let end_row = (m.end_row as isize + delta).max(0) as usize;
                return Some((row, end_row));
            }
            None
        });
    }
}

/// Engine-wide decoration registry: namespaces, named highlight groups, and
/// every buffer's [`BufferDecorations`]. One instance lives on `Engine`
/// (`Engine::decor`); every method below is a thin, buffer-keyed dispatch
/// onto it, called from both `engine/buffers.rs` (the edit-shifting hooks)
/// and `plugin.rs`'s `vimcode.decor.*` Lua bindings.
#[derive(Debug, Clone, Default)]
pub struct DecorState {
    namespaces: HashMap<String, NamespaceId>,
    next_ns: u32,
    next_mark: u64,
    pub highlight_groups: HashMap<String, HlGroupDef>,
    buffers: HashMap<BufferId, BufferDecorations>,
}

impl DecorState {
    /// `vimcode.decor.namespace(name)`: idempotent by `name` — calling it
    /// again with the same string returns the same id, so a plugin can call
    /// it on every load without accumulating namespaces.
    pub fn namespace(&mut self, name: &str) -> NamespaceId {
        if let Some(&id) = self.namespaces.get(name) {
            return id;
        }
        let id = NamespaceId(self.next_ns);
        self.next_ns += 1;
        self.namespaces.insert(name.to_string(), id);
        id
    }

    pub fn set_hl(&mut self, name: &str, def: HlGroupDef) {
        self.highlight_groups.insert(name.to_string(), def);
    }

    /// Resolve `name` through at most one `link` hop. `None` when `name`
    /// isn't a registered group.
    pub fn resolve_hl(&self, name: &str) -> Option<&HlGroupDef> {
        let def = self.highlight_groups.get(name)?;
        if let Some(link) = &def.link {
            if let Some(linked) = self.highlight_groups.get(link) {
                return Some(linked);
            }
        }
        Some(def)
    }

    fn buf_mut(&mut self, buf: BufferId) -> &mut BufferDecorations {
        self.buffers.entry(buf).or_default()
    }

    /// `vimcode.decor.set_mark`. `end_row`/`end_col` default to `row`/`col`
    /// (a point mark); an inverted range (`end` before `start`) is clamped to
    /// a point at `(row, col)` rather than silently doing something a caller
    /// didn't ask for.
    #[allow(clippy::too_many_arguments)]
    pub fn set_mark(
        &mut self,
        buf: BufferId,
        ns: NamespaceId,
        row: usize,
        col: usize,
        end_row: Option<usize>,
        end_col: Option<usize>,
        opts: DecorOpts,
    ) -> MarkId {
        let id = MarkId(self.next_mark);
        self.next_mark += 1;
        let mut end_row = end_row.unwrap_or(row);
        let mut end_col = end_col.unwrap_or(col);
        if end_row < row || (end_row == row && end_col < col) {
            end_row = row;
            end_col = col;
        }
        let mark = DecorMark {
            id,
            ns,
            row,
            col,
            end_row,
            end_col,
            opts,
        };
        self.buf_mut(buf).insert(mark);
        id
    }

    /// `None` both when `id` doesn't exist and when it exists but belongs to
    /// a different namespace than `ns` — a plugin can only read back its own
    /// marks, the same isolation `clear` enforces.
    pub fn get_mark(&self, buf: BufferId, ns: NamespaceId, id: MarkId) -> Option<&DecorMark> {
        self.buffers.get(&buf)?.get(id).filter(|m| m.ns == ns)
    }

    /// `false` when `id` doesn't exist or belongs to a different namespace —
    /// see [`get_mark`](Self::get_mark).
    pub fn del_mark(&mut self, buf: BufferId, ns: NamespaceId, id: MarkId) -> bool {
        let Some(b) = self.buffers.get_mut(&buf) else {
            return false;
        };
        if b.get(id).is_some_and(|m| m.ns == ns) {
            b.remove(id).is_some()
        } else {
            false
        }
    }

    pub fn clear(&mut self, buf: BufferId, ns: NamespaceId, range: Option<(usize, usize)>) {
        if let Some(b) = self.buffers.get_mut(&buf) {
            b.clear_ns(ns, range);
        }
    }

    /// Marks touching visible rows `[start_row, end_row]` in `buf` — what a
    /// renderer calls once per frame, per visible buffer.
    pub fn marks_touching(
        &self,
        buf: BufferId,
        start_row: usize,
        end_row: usize,
    ) -> Vec<&DecorMark> {
        self.buffers
            .get(&buf)
            .map(|b| b.marks_touching(start_row, end_row))
            .unwrap_or_default()
    }

    pub fn shift_insert(
        &mut self,
        buf: BufferId,
        at_line: usize,
        line_count: usize,
        at_line_start: bool,
    ) {
        if let Some(b) = self.buffers.get_mut(&buf) {
            b.shift_insert(at_line, line_count, at_line_start);
        }
    }

    pub fn shift_delete(&mut self, buf: BufferId, at_line: usize, line_count: usize) {
        if let Some(b) = self.buffers.get_mut(&buf) {
            b.shift_delete(at_line, line_count);
        }
    }

    pub fn shift_for_text_replace(&mut self, buf: BufferId, old_text: &str, new_text: &str) {
        if let Some(b) = self.buffers.get_mut(&buf) {
            b.shift_for_text_replace(old_text, new_text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_editing() {
        let mut buffer = Buffer::new(BufferId(1));
        buffer.insert(0, "Hello");
        assert_eq!(buffer.to_string(), "Hello");

        buffer.insert(5, " World");
        assert_eq!(buffer.to_string(), "Hello World");

        buffer.delete_range(5, 11);
        assert_eq!(buffer.to_string(), "Hello");
    }

    /// #1560: opening a UTF-16LE-with-BOM file (Notepad's/PowerShell's
    /// "Unicode" default on Windows) must decode correctly instead of
    /// erroring with "stream did not contain valid UTF-8". Against the old
    /// `fs::read_to_string`-based `from_file`, this test observably fails:
    /// `read_to_string` rejects the BOM bytes outright, so `from_file`
    /// returns `Err`, not the decoded "12345" this asserts on.
    #[test]
    fn test_from_file_utf16le_bom() {
        let path = std::env::temp_dir().join("vimcode_buffer_test_utf16le_bom.txt");
        let mut bytes: Vec<u8> = vec![0xFF, 0xFE]; // UTF-16LE BOM
        for unit in "12345".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();

        let buffer = Buffer::from_file(BufferId(1), &path).unwrap();
        assert_eq!(buffer.to_string(), "12345");

        let _ = fs::remove_file(&path);
    }

    /// #1560: mirror of the LE case for a UTF-16BE-with-BOM file.
    #[test]
    fn test_from_file_utf16be_bom() {
        let path = std::env::temp_dir().join("vimcode_buffer_test_utf16be_bom.txt");
        let mut bytes: Vec<u8> = vec![0xFE, 0xFF]; // UTF-16BE BOM
        for unit in "hello".encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        fs::write(&path, &bytes).unwrap();

        let buffer = Buffer::from_file(BufferId(1), &path).unwrap();
        assert_eq!(buffer.to_string(), "hello");

        let _ = fs::remove_file(&path);
    }

    /// #1560: a UTF-8-with-BOM file (`EF BB BF` prefix) must decode with
    /// the BOM stripped rather than surfacing it as a stray character (or,
    /// under a stricter reader, an error) in the buffer content.
    #[test]
    fn test_from_file_utf8_bom() {
        let path = std::env::temp_dir().join("vimcode_buffer_test_utf8_bom.txt");
        let mut bytes: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hello world".as_bytes());
        fs::write(&path, &bytes).unwrap();

        let buffer = Buffer::from_file(BufferId(1), &path).unwrap();
        assert_eq!(buffer.to_string(), "hello world");

        let _ = fs::remove_file(&path);
    }

    /// A plain UTF-8 file with no BOM must keep working exactly as before.
    #[test]
    fn test_from_file_plain_utf8() {
        let path = std::env::temp_dir().join("vimcode_buffer_test_plain_utf8.txt");
        fs::write(&path, "no bom here\n").unwrap();

        let buffer = Buffer::from_file(BufferId(1), &path).unwrap();
        assert_eq!(buffer.to_string(), "no bom here\n");

        let _ = fs::remove_file(&path);
    }

    /// A file that is neither valid UTF-8 nor a recognized BOM'd encoding
    /// must still surface a clear `io::Error` rather than silently
    /// succeeding with mangled content.
    #[test]
    fn test_from_file_invalid_utf8_errors() {
        let path = std::env::temp_dir().join("vimcode_buffer_test_invalid_utf8.txt");
        fs::write(&path, [0xFF, 0x00, 0xFF, 0x01]).unwrap();

        let result = Buffer::from_file(BufferId(1), &path);
        assert!(result.is_err());

        let _ = fs::remove_file(&path);
    }

    /// A UTF-32LE-with-BOM file (`FF FE 00 00`) shares its first two bytes
    /// with the UTF-16LE BOM (`FF FE`). Without an explicit UTF-32 check
    /// ahead of the UTF-16LE one, this would be misdetected as UTF-16LE and
    /// decoded into garbage instead of surfacing the documented "unsupported
    /// encoding" `io::Error`.
    #[test]
    fn test_from_file_utf32le_bom_errors_instead_of_misdecoding() {
        let path = std::env::temp_dir().join("vimcode_buffer_test_utf32le_bom.txt");
        let mut bytes: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x00]; // UTF-32LE BOM
        for ch in "hi".chars() {
            bytes.extend_from_slice(&(ch as u32).to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();

        let result = Buffer::from_file(BufferId(1), &path);
        assert!(
            result.is_err(),
            "UTF-32LE must surface a clear error, not misdecoded content"
        );

        let _ = fs::remove_file(&path);
    }
}
