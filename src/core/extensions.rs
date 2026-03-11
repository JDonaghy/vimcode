//! Extension manifest data model.
//!
//! Extensions bundle an LSP server, optional DAP adapter, and optional Lua
//! scripts into a single named package. Users install them with `:ExtInstall`.
//!
//! Manifests are fetched from a remote registry (GitHub) and cached locally.
//! There are no compiled-in extensions — the registry is the single source of
//! truth.

use serde::{Deserialize, Serialize};

// ─── Manifest deserialization ─────────────────────────────────────────────────

/// Parsed contents of a `manifest.toml` (or registry JSON entry).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ExtensionManifest {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    /// File extensions this extension activates for (e.g. `[".cs"]`).
    #[serde(default)]
    pub file_extensions: Vec<String>,
    /// LSP language IDs (e.g. `["csharp"]`).
    #[serde(default)]
    pub language_ids: Vec<String>,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    pub dap: DapConfig,
    /// Lua script filenames bundled with this extension.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Files/directories whose presence indicates this language's project root.
    /// E.g. `["Cargo.toml"]` for Rust, `["go.mod"]` for Go.
    #[serde(default)]
    pub workspace_markers: Vec<String>,
    /// Optional comment style override for languages handled by this extension.
    #[serde(default)]
    pub comment: Option<CommentConfig>,
}

/// Comment style override specified in an extension manifest `[comment]` section.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CommentConfig {
    #[serde(default)]
    pub line: String,
    #[serde(default)]
    pub block_open: String,
    #[serde(default)]
    pub block_close: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LspConfig {
    /// Binary name that must be on PATH (e.g. `"csharp-ls"`).
    #[serde(default)]
    pub binary: String,
    /// Shell command to install the LSP server.
    #[serde(default)]
    pub install: String,
    /// Fallback binaries tried in order when `binary` is not found on PATH.
    /// E.g. `["basedpyright-langserver", "pylsp", "jedi-language-server"]` for Python.
    #[serde(default)]
    pub fallback_binaries: Vec<String>,
    /// Command-line arguments passed to the LSP binary (default: `["--stdio"]` if needed).
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DapConfig {
    /// Adapter name matching `dap_manager`'s registry (e.g. `"netcoredbg"`).
    #[serde(default)]
    pub adapter: String,
    /// Executable to launch for the DAP adapter (e.g. `"codelldb"`, `"python"`).
    #[serde(default)]
    pub binary: String,
    /// Shell command to install the DAP adapter.
    #[serde(default)]
    pub install: String,
    /// Transport protocol: `"stdio"` (default) or `"tcp"`.
    #[serde(default)]
    pub transport: String,
    /// Arguments passed to the DAP binary.
    #[serde(default)]
    pub args: Vec<String>,
}

impl ExtensionManifest {
    /// Parse a manifest from a TOML string. Returns `None` on parse failure.
    #[allow(dead_code)]
    pub fn parse(toml: &str) -> Option<Self> {
        toml::from_str(toml).ok()
    }

    /// Returns true if this extension is relevant for the given file extension
    /// (e.g. ".cs") or language ID.
    #[allow(dead_code)]
    pub fn matches_file_ext(&self, ext: &str) -> bool {
        self.file_extensions.iter().any(|e| e == ext)
    }

    pub fn matches_language_id(&self, lang: &str) -> bool {
        self.language_ids.iter().any(|l| l == lang)
    }
}

// ─── Lookup helpers (operate on a slice of manifests) ─────────────────────────

/// Find a manifest by name (case-insensitive) in a slice.
#[allow(dead_code)]
pub fn find_manifest_by_name<'a>(
    manifests: &'a [ExtensionManifest],
    name: &str,
) -> Option<&'a ExtensionManifest> {
    manifests.iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Find the first manifest whose `file_extensions` list contains `ext`.
#[allow(dead_code)]
pub fn find_manifest_for_file_ext<'a>(
    manifests: &'a [ExtensionManifest],
    ext: &str,
) -> Option<&'a ExtensionManifest> {
    manifests.iter().find(|m| m.matches_file_ext(ext))
}

/// Find the first manifest whose `language_ids` list contains `lang`.
pub fn find_manifest_for_language_id<'a>(
    manifests: &'a [ExtensionManifest],
    lang: &str,
) -> Option<&'a ExtensionManifest> {
    manifests.iter().find(|m| m.matches_language_id(lang))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifests() -> Vec<ExtensionManifest> {
        vec![
            ExtensionManifest {
                name: "rust".to_string(),
                display_name: "Rust Language Support".to_string(),
                file_extensions: vec![".rs".to_string()],
                language_ids: vec!["rust".to_string()],
                lsp: LspConfig {
                    binary: "rust-analyzer".to_string(),
                    ..Default::default()
                },
                dap: DapConfig {
                    adapter: "codelldb".to_string(),
                    binary: "codelldb".to_string(),
                    transport: "tcp".to_string(),
                    args: vec!["--port".to_string(), "0".to_string()],
                    ..Default::default()
                },
                workspace_markers: vec!["Cargo.toml".to_string()],
                ..Default::default()
            },
            ExtensionManifest {
                name: "python".to_string(),
                display_name: "Python Language Support".to_string(),
                file_extensions: vec![".py".to_string(), ".pyi".to_string()],
                language_ids: vec!["python".to_string()],
                lsp: LspConfig {
                    binary: "pyright-langserver".to_string(),
                    fallback_binaries: vec!["pylsp".to_string()],
                    args: vec!["--stdio".to_string()],
                    ..Default::default()
                },
                workspace_markers: vec!["pyproject.toml".to_string()],
                ..Default::default()
            },
            ExtensionManifest {
                name: "git-insights".to_string(),
                display_name: "Git Insights".to_string(),
                scripts: vec!["blame.lua".to_string()],
                ..Default::default()
            },
        ]
    }

    #[test]
    fn find_by_name_case_insensitive() {
        let ms = sample_manifests();
        assert!(find_manifest_by_name(&ms, "rust").is_some());
        assert!(find_manifest_by_name(&ms, "Rust").is_some());
        assert!(find_manifest_by_name(&ms, "RUST").is_some());
        assert!(find_manifest_by_name(&ms, "nonexistent").is_none());
    }

    #[test]
    fn find_by_file_ext() {
        let ms = sample_manifests();
        let m = find_manifest_for_file_ext(&ms, ".rs").expect(".rs should match rust");
        assert_eq!(m.name, "rust");
        let m = find_manifest_for_file_ext(&ms, ".py").expect(".py should match python");
        assert_eq!(m.name, "python");
        assert!(find_manifest_for_file_ext(&ms, ".xyz").is_none());
    }

    #[test]
    fn find_by_language_id() {
        let ms = sample_manifests();
        let m = find_manifest_for_language_id(&ms, "rust").expect("rust lang id");
        assert_eq!(m.name, "rust");
        let m = find_manifest_for_language_id(&ms, "python").expect("python lang id");
        assert_eq!(m.name, "python");
        assert!(find_manifest_for_language_id(&ms, "cobol").is_none());
    }

    #[test]
    fn manifest_parse_toml() {
        let toml = r#"
name = "test"
display_name = "Test Extension"
file_extensions = [".test"]
language_ids = ["test"]
[lsp]
binary = "test-lsp"
"#;
        let m = ExtensionManifest::parse(toml).expect("should parse");
        assert_eq!(m.name, "test");
        assert_eq!(m.lsp.binary, "test-lsp");
    }

    #[test]
    fn manifest_serialize_deserialize_json_roundtrip() {
        let ms = sample_manifests();
        let json = serde_json::to_string(&ms).expect("serialize");
        let back: Vec<ExtensionManifest> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.len(), ms.len());
        assert_eq!(back[0].name, "rust");
        assert_eq!(back[1].lsp.fallback_binaries, vec!["pylsp"]);
    }
}
