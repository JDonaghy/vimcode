//! Built-in extension manifests and the extension data model.
//!
//! Extensions bundle an LSP server, optional DAP adapter, and optional Lua
//! scripts into a single named package. Users install them with `:ExtInstall`.

// Manifest fields are populated via Serde deserialization; not all are
// accessed from Rust code directly, hence the allow below.
#![allow(dead_code)]

use serde::Deserialize;

// ─── Bundled extension data (compile-time) ────────────────────────────────────

/// A single compiled-in extension.
pub struct BundledExtension {
    /// Short identifier used in `:ExtInstall`, `:ExtList`, etc.
    pub name: &'static str,
    /// Raw TOML content of `extensions/<name>/manifest.toml`.
    pub manifest_toml: &'static str,
    /// Lua scripts bundled with the extension: `(filename, source)`.
    pub scripts: &'static [(&'static str, &'static str)],
    /// README markdown bundled with the extension.
    pub readme: &'static str,
}

/// All extensions compiled into the binary.
pub static BUNDLED: &[BundledExtension] = &[
    BundledExtension {
        name: "csharp",
        manifest_toml: include_str!("../../extensions/csharp/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/csharp/README.md"),
    },
    BundledExtension {
        name: "python",
        manifest_toml: include_str!("../../extensions/python/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/python/README.md"),
    },
    BundledExtension {
        name: "rust",
        manifest_toml: include_str!("../../extensions/rust/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/rust/README.md"),
    },
    BundledExtension {
        name: "javascript",
        manifest_toml: include_str!("../../extensions/javascript/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/javascript/README.md"),
    },
    BundledExtension {
        name: "go",
        manifest_toml: include_str!("../../extensions/go/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/go/README.md"),
    },
    BundledExtension {
        name: "java",
        manifest_toml: include_str!("../../extensions/java/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/java/README.md"),
    },
    BundledExtension {
        name: "cpp",
        manifest_toml: include_str!("../../extensions/cpp/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/cpp/README.md"),
    },
    BundledExtension {
        name: "php",
        manifest_toml: include_str!("../../extensions/php/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/php/README.md"),
    },
    BundledExtension {
        name: "ruby",
        manifest_toml: include_str!("../../extensions/ruby/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/ruby/README.md"),
    },
    BundledExtension {
        name: "bash",
        manifest_toml: include_str!("../../extensions/bash/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/bash/README.md"),
    },
    BundledExtension {
        name: "json",
        manifest_toml: include_str!("../../extensions/json/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/json/README.md"),
    },
    BundledExtension {
        name: "xml",
        manifest_toml: include_str!("../../extensions/xml/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/xml/README.md"),
    },
    BundledExtension {
        name: "yaml",
        manifest_toml: include_str!("../../extensions/yaml/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/yaml/README.md"),
    },
    BundledExtension {
        name: "markdown",
        manifest_toml: include_str!("../../extensions/markdown/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/markdown/README.md"),
    },
    BundledExtension {
        name: "bicep",
        manifest_toml: include_str!("../../extensions/bicep/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/bicep/README.md"),
    },
    BundledExtension {
        name: "terraform",
        manifest_toml: include_str!("../../extensions/terraform/manifest.toml"),
        scripts: &[],
        readme: include_str!("../../extensions/terraform/README.md"),
    },
    BundledExtension {
        name: "git-insights",
        manifest_toml: include_str!("../../extensions/git-insights/manifest.toml"),
        scripts: &[(
            "blame.lua",
            include_str!("../../extensions/git-insights/blame.lua"),
        )],
        readme: include_str!("../../extensions/git-insights/README.md"),
    },
];

// ─── Manifest deserialization ─────────────────────────────────────────────────

/// Parsed contents of a `manifest.toml`.
#[derive(Debug, Clone, Deserialize, Default)]
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
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CommentConfig {
    #[serde(default)]
    pub line: String,
    #[serde(default)]
    pub block_open: String,
    #[serde(default)]
    pub block_close: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
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

#[derive(Debug, Clone, Deserialize, Default)]
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
    pub fn parse(toml: &str) -> Option<Self> {
        toml::from_str(toml).ok()
    }

    /// Returns true if this extension is relevant for the given file extension
    /// (e.g. ".cs") or language ID.
    pub fn matches_file_ext(&self, ext: &str) -> bool {
        self.file_extensions.iter().any(|e| e == ext)
    }

    pub fn matches_language_id(&self, lang: &str) -> bool {
        self.language_ids.iter().any(|l| l == lang)
    }
}

// ─── Lookup helpers ───────────────────────────────────────────────────────────

/// Find a bundled extension by name (case-insensitive).
pub fn find_by_name(name: &str) -> Option<&'static BundledExtension> {
    BUNDLED.iter().find(|e| e.name.eq_ignore_ascii_case(name))
}

/// Find the first bundled extension whose file_extensions list contains `ext`
/// (e.g. `".cs"`).
pub fn find_for_file_ext(ext: &str) -> Option<(&'static BundledExtension, ExtensionManifest)> {
    for bundle in BUNDLED {
        if let Some(manifest) = ExtensionManifest::parse(bundle.manifest_toml) {
            if manifest.matches_file_ext(ext) {
                return Some((bundle, manifest));
            }
        }
    }
    None
}

/// Find the first bundled extension whose language_ids list contains `lang`.
pub fn find_for_language_id(lang: &str) -> Option<(&'static BundledExtension, ExtensionManifest)> {
    for bundle in BUNDLED {
        if let Some(manifest) = ExtensionManifest::parse(bundle.manifest_toml) {
            if manifest.matches_language_id(lang) {
                return Some((bundle, manifest));
            }
        }
    }
    None
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_bundled_manifests_parse() {
        for b in BUNDLED {
            let m = ExtensionManifest::parse(b.manifest_toml)
                .unwrap_or_else(|| panic!("failed to parse manifest for '{}'", b.name));
            assert_eq!(m.name, b.name, "name mismatch in {}", b.name);
            assert!(
                !m.display_name.is_empty(),
                "missing display_name in {}",
                b.name
            );
        }
    }

    #[test]
    fn find_csharp_by_name() {
        let b = find_by_name("csharp").expect("csharp not found");
        assert_eq!(b.name, "csharp");
    }

    #[test]
    fn find_csharp_by_file_ext() {
        let (b, m) = find_for_file_ext(".cs").expect(".cs not found");
        assert_eq!(b.name, "csharp");
        assert_eq!(m.lsp.binary, "csharp-ls");
        assert_eq!(m.dap.adapter, "netcoredbg");
    }

    #[test]
    fn find_by_language_id() {
        let (b, _) = find_for_language_id("python").expect("python not found");
        assert_eq!(b.name, "python");
    }

    #[test]
    fn git_insights_has_script() {
        let b = find_by_name("git-insights").expect("git-insights not found");
        assert_eq!(b.scripts.len(), 1);
        assert_eq!(b.scripts[0].0, "blame.lua");
        assert!(!b.scripts[0].1.is_empty());
    }

    #[test]
    fn find_nonexistent_returns_none() {
        assert!(find_by_name("nonexistent").is_none());
        assert!(find_for_file_ext(".xyz").is_none());
    }

    // ── New manifest fields ───────────────────────────────────────────────────

    #[test]
    fn python_manifest_has_fallback_binaries() {
        let (_, m) = find_for_language_id("python").expect("python manifest");
        assert!(
            !m.lsp.fallback_binaries.is_empty(),
            "python should have LSP fallback binaries"
        );
        assert!(
            m.lsp.fallback_binaries.contains(&"pylsp".to_string()),
            "python fallbacks should include pylsp: {:?}",
            m.lsp.fallback_binaries
        );
    }

    #[test]
    fn python_manifest_has_lsp_args() {
        let (_, m) = find_for_language_id("python").expect("python manifest");
        assert_eq!(
            m.lsp.args,
            vec!["--stdio"],
            "python lsp should use --stdio arg"
        );
    }

    #[test]
    fn python_manifest_has_dap_binary_and_args() {
        let (_, m) = find_for_language_id("python").expect("python manifest");
        assert_eq!(m.dap.binary, "python", "python dap binary should be python");
        assert_eq!(m.dap.transport, "stdio");
        assert!(
            m.dap.args.contains(&"-m".to_string())
                && m.dap.args.contains(&"debugpy.adapter".to_string()),
            "python dap args should include -m debugpy.adapter: {:?}",
            m.dap.args
        );
    }

    #[test]
    fn python_manifest_has_workspace_markers() {
        let (_, m) = find_for_language_id("python").expect("python manifest");
        assert!(
            !m.workspace_markers.is_empty(),
            "python should have workspace_markers"
        );
        assert!(
            m.workspace_markers.contains(&"pyproject.toml".to_string()),
            "python workspace_markers should include pyproject.toml"
        );
    }

    #[test]
    fn rust_manifest_has_dap_config() {
        let (_, m) = find_for_language_id("rust").expect("rust manifest");
        assert_eq!(m.dap.adapter, "codelldb");
        assert_eq!(m.dap.binary, "codelldb");
        assert_eq!(m.dap.transport, "tcp");
        assert!(
            m.workspace_markers.contains(&"Cargo.toml".to_string()),
            "rust should have Cargo.toml as workspace marker"
        );
    }

    #[test]
    fn go_manifest_has_dap_config() {
        let (_, m) = find_for_language_id("go").expect("go manifest");
        assert_eq!(m.dap.adapter, "delve");
        assert_eq!(m.dap.binary, "dlv");
        assert_eq!(m.dap.transport, "stdio");
        assert_eq!(m.dap.args, vec!["dap"]);
        assert!(!m.dap.install.is_empty(), "go dap should have install cmd");
        assert!(
            m.workspace_markers.contains(&"go.mod".to_string()),
            "go should have go.mod as workspace marker"
        );
    }

    #[test]
    fn csharp_manifest_has_dap_config() {
        let (_, m) = find_for_language_id("csharp").expect("csharp manifest");
        assert_eq!(m.dap.adapter, "netcoredbg");
        assert_eq!(m.dap.binary, "netcoredbg");
        assert_eq!(m.dap.transport, "stdio");
        assert!(
            m.dap.args.contains(&"--interpreter=vscode".to_string()),
            "netcoredbg args should include --interpreter=vscode"
        );
    }

    #[test]
    fn javascript_manifest_has_dap_config() {
        let (_, m) = find_for_language_id("javascript").expect("javascript manifest");
        assert_eq!(m.dap.adapter, "js-debug");
        assert!(
            m.workspace_markers.contains(&"package.json".to_string()),
            "javascript should have package.json as workspace marker"
        );
    }

    #[test]
    fn all_manifests_parse_with_new_fields() {
        for b in BUNDLED {
            let m = ExtensionManifest::parse(b.manifest_toml)
                .unwrap_or_else(|| panic!("failed to parse manifest for '{}'", b.name));
            // New fields should deserialize correctly (even if empty/default).
            let _ = m.lsp.fallback_binaries;
            let _ = m.lsp.args;
            let _ = m.dap.binary;
            let _ = m.dap.install;
            let _ = m.dap.transport;
            let _ = m.dap.args;
            let _ = m.workspace_markers;
        }
    }
}
