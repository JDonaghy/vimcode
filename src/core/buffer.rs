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
