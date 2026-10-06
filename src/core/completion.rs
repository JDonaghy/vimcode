//! Insert-mode completion popup candidates (#1805).
//!
//! `Engine::completion_candidates` used to be a bare `Vec<String>` — buffer-
//! word and LSP completion never needed anything richer than "text to
//! insert". A `vimcode.completion.register` plugin source (#1805) can supply
//! an icon/kind, right-aligned detail text and a documentation string, so
//! the popup needs a real item shape to carry them.
//!
//! [`CompletionItemKind`] is vimcode's own vocabulary, not quadraui's —
//! mirrors the rule in `plugin_ui`'s module doc ("a vimcode-owned vocabulary,
//! converted to quadraui primitives in one place"): the Lua-facing strings
//! and this enum are pinned by `from_str_lenient` below, independent of
//! whatever quadraui's `CompletionKind` looks like today. `render.rs`'s
//! `completion_menu_to_quadraui_completions` is the one place that maps one
//! to the other.

/// Kind of a completion candidate — drives the popup's icon/colour once
/// painted through the quadraui `Completions` primitive. Mirrors (a subset
/// of) LSP's `CompletionItemKind` naming, since that's the vocabulary most
/// plugin authors (and the LSP client itself, eventually) already know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompletionItemKind {
    #[default]
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Color,
    File,
    Reference,
    Folder,
    EnumMember,
    Constant,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

impl CompletionItemKind {
    /// Parse a `vimcode.completion.register`/`:complete()` item's `kind`
    /// string. Case-insensitive; an unrecognised value falls back to
    /// `Text` rather than erroring the whole call — the same leniency
    /// `vimcode.picker.open`'s `icon` field documents (#1630 review): a
    /// cosmetic field should not be able to break a plugin on a typo.
    pub fn from_str_lenient(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "method" => Self::Method,
            "function" => Self::Function,
            "constructor" => Self::Constructor,
            "field" => Self::Field,
            "variable" => Self::Variable,
            "class" => Self::Class,
            "interface" => Self::Interface,
            "module" => Self::Module,
            "property" => Self::Property,
            "unit" => Self::Unit,
            "value" => Self::Value,
            "enum" => Self::Enum,
            "keyword" => Self::Keyword,
            "snippet" => Self::Snippet,
            "color" => Self::Color,
            "file" => Self::File,
            "reference" => Self::Reference,
            "folder" => Self::Folder,
            "enummember" | "enum_member" => Self::EnumMember,
            "constant" => Self::Constant,
            "struct" => Self::Struct,
            "event" => Self::Event,
            "operator" => Self::Operator,
            "typeparameter" | "type_parameter" => Self::TypeParameter,
            _ => Self::Text,
        }
    }
}

/// One completion-popup candidate. Buffer-word and LSP completion populate
/// only `label`/`insert_text` (default `kind`/`detail`/`documentation`,
/// `priority` 0) — unchanged behaviour from before #1805. A
/// `vimcode.completion.register` source can also set `kind`/`detail`/
/// `documentation`/`priority`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompletionCandidate {
    /// Display label (what the user sees in the popup).
    pub label: String,
    /// Text actually inserted into the buffer on accept. Usually equal to
    /// `label`; LSP/plugin sources can differ (e.g. label `"map(..)"`
    /// inserts `"map"`).
    pub insert_text: String,
    pub kind: CompletionItemKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    /// Sort weight relative to buffer-word/LSP candidates, which are always
    /// priority `0`. Only a plugin source sets this to non-zero — higher
    /// sorts earlier in the popup (#1805's "priority relative to
    /// buffer/LSP candidates" requirement).
    pub priority: i32,
}

impl CompletionCandidate {
    /// A plain string candidate — what every buffer-word/LSP completion
    /// was before #1805 (`kind = Text`, no detail/documentation, priority
    /// `0`).
    pub fn plain(text: String) -> Self {
        Self {
            label: text.clone(),
            insert_text: text,
            ..Default::default()
        }
    }
}

// `c == "foobar"` comparisons against the *insert text* let the existing
// buffer-word/LSP completion tests (predating #1805) keep comparing plain
// string literals instead of constructing a `CompletionCandidate` at every
// assertion site.
impl PartialEq<str> for CompletionCandidate {
    fn eq(&self, other: &str) -> bool {
        self.insert_text == other
    }
}

impl PartialEq<String> for CompletionCandidate {
    fn eq(&self, other: &String) -> bool {
        self.insert_text == *other
    }
}

impl PartialEq<&str> for CompletionCandidate {
    fn eq(&self, other: &&str) -> bool {
        self.insert_text == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_sets_label_and_insert_text_to_the_same_string() {
        let c = CompletionCandidate::plain("foobar".to_string());
        assert_eq!(c.label, "foobar");
        assert_eq!(c.insert_text, "foobar");
        assert_eq!(c.kind, CompletionItemKind::Text);
        assert_eq!(c.priority, 0);
    }

    #[test]
    fn eq_str_compares_insert_text() {
        let c = CompletionCandidate {
            label: "map(..)".to_string(),
            insert_text: "map".to_string(),
            ..Default::default()
        };
        assert_eq!(c, "map".to_string());
        assert!(c != "map(..)".to_string());
    }

    #[test]
    fn from_str_lenient_falls_back_to_text_for_unknown_kind() {
        assert_eq!(
            CompletionItemKind::from_str_lenient("bogus"),
            CompletionItemKind::Text
        );
        assert_eq!(
            CompletionItemKind::from_str_lenient("Function"),
            CompletionItemKind::Function
        );
    }
}
