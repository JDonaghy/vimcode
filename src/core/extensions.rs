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
}

/// All extensions compiled into the binary.
pub static BUNDLED: &[BundledExtension] = &[
    BundledExtension {
        name: "csharp",
        manifest_toml: include_str!("../../extensions/csharp/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "python",
        manifest_toml: include_str!("../../extensions/python/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "rust",
        manifest_toml: include_str!("../../extensions/rust/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "javascript",
        manifest_toml: include_str!("../../extensions/javascript/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "go",
        manifest_toml: include_str!("../../extensions/go/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "java",
        manifest_toml: include_str!("../../extensions/java/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "cpp",
        manifest_toml: include_str!("../../extensions/cpp/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "php",
        manifest_toml: include_str!("../../extensions/php/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "ruby",
        manifest_toml: include_str!("../../extensions/ruby/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "bash",
        manifest_toml: include_str!("../../extensions/bash/manifest.toml"),
        scripts: &[],
    },
    BundledExtension {
        name: "git-insights",
        manifest_toml: include_str!("../../extensions/git-insights/manifest.toml"),
        scripts: &[(
            "blame.lua",
            include_str!("../../extensions/git-insights/blame.lua"),
        )],
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
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LspConfig {
    /// Binary name that must be on PATH (e.g. `"csharp-ls"`).
    #[serde(default)]
    pub binary: String,
    /// Shell command to install the LSP server.
    #[serde(default)]
    pub install: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DapConfig {
    /// Adapter name matching `dap_manager`'s registry (e.g. `"netcoredbg"`).
    #[serde(default)]
    pub adapter: String,
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
}
