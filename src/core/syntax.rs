use tree_sitter::{Language, Parser, Point, Query, QueryCursor, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxLanguage {
    Rust,
    Python,
    JavaScript,
    Go,
    Cpp,
    C,
    TypeScript,
    TypeScriptReact,
    // TODO: Html — tree-sitter-html 0.20.4 depends on tree-sitter 0.22, incompatible
    Css,
    Json,
    Bash,
    Ruby,
    CSharp,
    Java,
    Toml,
    // TODO: Lua (tree-sitter-lua has no 0.20.x release on crates.io)
    // TODO: Kotlin (tree-sitter-kotlin has no 0.20.x release on crates.io)
}

impl SyntaxLanguage {
    /// Detect language from file extension
    pub fn from_path(path: &str) -> Option<Self> {
        let path_lower = path.to_lowercase();

        if path_lower.ends_with(".rs") {
            Some(Self::Rust)
        } else if path_lower.ends_with(".py") || path_lower.ends_with(".pyw") {
            Some(Self::Python)
        } else if path_lower.ends_with(".tsx") {
            Some(Self::TypeScriptReact)
        } else if path_lower.ends_with(".ts") {
            Some(Self::TypeScript)
        } else if path_lower.ends_with(".js")
            || path_lower.ends_with(".jsx")
            || path_lower.ends_with(".mjs")
            || path_lower.ends_with(".cjs")
        {
            Some(Self::JavaScript)
        } else if path_lower.ends_with(".go") {
            Some(Self::Go)
        } else if path_lower.ends_with(".cpp")
            || path_lower.ends_with(".cc")
            || path_lower.ends_with(".cxx")
            || path_lower.ends_with(".c++")
            || path_lower.ends_with(".hpp")
            || path_lower.ends_with(".hh")
            || path_lower.ends_with(".hxx")
        {
            Some(Self::Cpp)
        } else if path_lower.ends_with(".c") || path_lower.ends_with(".h") {
            Some(Self::C)
        } else if path_lower.ends_with(".css") {
            Some(Self::Css)
        } else if path_lower.ends_with(".json") || path_lower.ends_with(".jsonc") {
            Some(Self::Json)
        } else if path_lower.ends_with(".sh")
            || path_lower.ends_with(".bash")
            || path_lower.ends_with(".zsh")
        {
            Some(Self::Bash)
        } else if path_lower.ends_with(".rb") {
            Some(Self::Ruby)
        } else if path_lower.ends_with(".cs") {
            Some(Self::CSharp)
        } else if path_lower.ends_with(".java") {
            Some(Self::Java)
        } else if path_lower.ends_with(".toml") {
            Some(Self::Toml)
        } else {
            None
        }
    }

    fn language(&self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::language(),
            Self::Python => tree_sitter_python::language(),
            Self::JavaScript => tree_sitter_javascript::language(),
            Self::Go => tree_sitter_go::language(),
            Self::Cpp => tree_sitter_cpp::language(),
            Self::C => tree_sitter_c::language(),
            Self::TypeScript => tree_sitter_typescript::language_typescript(),
            Self::TypeScriptReact => tree_sitter_typescript::language_tsx(),
            Self::Css => tree_sitter_css::language(),
            Self::Json => tree_sitter_json::language(),
            Self::Bash => tree_sitter_bash::language(),
            Self::Ruby => tree_sitter_ruby::language(),
            Self::CSharp => tree_sitter_c_sharp::language(),
            Self::Java => tree_sitter_java::language(),
            Self::Toml => tree_sitter_toml::language(),
        }
    }

    fn query_source(&self) -> &'static str {
        match self {
            Self::Rust => "
                (function_item name: (identifier) @function)
                (string_literal) @string
                (line_comment) @comment
                (mod_item name: (identifier) @module)
                [
                  \"fn\"
                  \"struct\"
                  \"enum\"
                  \"impl\"
                  \"pub\"
                  \"use\"
                  \"mod\"
                  \"let\"
                  \"if\"
                  \"else\"
                  \"match\"
                ] @keyword
                (type_identifier) @type
                (primitive_type) @type
            ",
            Self::Python => "
                (function_definition name: (identifier) @function)
                (class_definition name: (identifier) @type)
                (string) @string
                (comment) @comment
                [
                  \"def\"
                  \"class\"
                  \"if\"
                  \"elif\"
                  \"else\"
                  \"for\"
                  \"while\"
                  \"return\"
                  \"import\"
                  \"from\"
                  \"as\"
                  \"try\"
                  \"except\"
                  \"finally\"
                  \"with\"
                  \"lambda\"
                  \"pass\"
                  \"break\"
                  \"continue\"
                  \"raise\"
                  \"yield\"
                  \"async\"
                  \"await\"
                ] @keyword
                (call function: (identifier) @function)
            ",
            Self::JavaScript => "
                (function_declaration name: (identifier) @function)
                (method_definition name: (property_identifier) @function)
                (class_declaration name: (identifier) @type)
                (string) @string
                (template_string) @string
                (comment) @comment
                [
                  \"function\"
                  \"class\"
                  \"const\"
                  \"let\"
                  \"var\"
                  \"if\"
                  \"else\"
                  \"for\"
                  \"while\"
                  \"do\"
                  \"return\"
                  \"import\"
                  \"export\"
                  \"from\"
                  \"default\"
                  \"try\"
                  \"catch\"
                  \"finally\"
                  \"throw\"
                  \"new\"
                  \"async\"
                  \"await\"
                  \"break\"
                  \"continue\"
                  \"switch\"
                  \"case\"
                ] @keyword
            ",
            Self::Go => "
                (function_declaration name: (identifier) @function)
                (method_declaration name: (field_identifier) @function)
                (type_declaration (type_spec name: (type_identifier) @type))
                (interpreted_string_literal) @string
                (raw_string_literal) @string
                (comment) @comment
                [
                  \"func\"
                  \"package\"
                  \"import\"
                  \"type\"
                  \"struct\"
                  \"interface\"
                  \"if\"
                  \"else\"
                  \"for\"
                  \"range\"
                  \"return\"
                  \"go\"
                  \"defer\"
                  \"var\"
                  \"const\"
                  \"switch\"
                  \"case\"
                  \"default\"
                  \"break\"
                  \"continue\"
                  \"fallthrough\"
                  \"select\"
                  \"chan\"
                  \"map\"
                ] @keyword
                (type_identifier) @type
            ",
            Self::Cpp => "
                (function_definition declarator: (function_declarator declarator: (identifier) @function))
                (declaration declarator: (function_declarator declarator: (identifier) @function))
                (class_specifier name: (type_identifier) @type)
                (struct_specifier name: (type_identifier) @type)
                (string_literal) @string
                (comment) @comment
                [
                  \"class\"
                  \"struct\"
                  \"enum\"
                  \"namespace\"
                  \"public\"
                  \"private\"
                  \"protected\"
                  \"virtual\"
                  \"static\"
                  \"const\"
                  \"if\"
                  \"else\"
                  \"for\"
                  \"while\"
                  \"do\"
                  \"return\"
                  \"break\"
                  \"continue\"
                  \"switch\"
                  \"case\"
                  \"default\"
                  \"template\"
                  \"typename\"
                  \"using\"
                  \"new\"
                  \"delete\"
                  \"try\"
                  \"catch\"
                  \"throw\"
                ] @keyword
                (type_identifier) @type
                (primitive_type) @type
            ",
            Self::C => "
                (function_definition declarator: (function_declarator declarator: (identifier) @function))
                (declaration declarator: (function_declarator declarator: (identifier) @function))
                (struct_specifier name: (type_identifier) @type)
                (enum_specifier name: (type_identifier) @type)
                (string_literal) @string
                (comment) @comment
                [
                  \"if\"
                  \"else\"
                  \"for\"
                  \"while\"
                  \"do\"
                  \"return\"
                  \"break\"
                  \"continue\"
                  \"switch\"
                  \"case\"
                  \"default\"
                  \"struct\"
                  \"enum\"
                  \"typedef\"
                  \"static\"
                  \"const\"
                  \"sizeof\"
                ] @keyword
                (type_identifier) @type
                (primitive_type) @type
            ",
            Self::TypeScript | Self::TypeScriptReact => "
                (function_declaration name: (identifier) @function)
                (method_definition name: (property_identifier) @function)
                (class_declaration name: (type_identifier) @type)
                (interface_declaration name: (type_identifier) @type)
                (type_alias_declaration name: (type_identifier) @type)
                (string) @string
                (template_string) @string
                (comment) @comment
                [
                  \"function\"
                  \"class\"
                  \"interface\"
                  \"type\"
                  \"const\"
                  \"let\"
                  \"var\"
                  \"if\"
                  \"else\"
                  \"for\"
                  \"while\"
                  \"do\"
                  \"return\"
                  \"import\"
                  \"export\"
                  \"from\"
                  \"default\"
                  \"try\"
                  \"catch\"
                  \"finally\"
                  \"throw\"
                  \"new\"
                  \"async\"
                  \"await\"
                  \"break\"
                  \"continue\"
                  \"switch\"
                  \"case\"
                  \"as\"
                  \"extends\"
                  \"implements\"
                ] @keyword
                (type_identifier) @type
            ",
            Self::Css => "
                (tag_name) @function
                (class_selector) @type
                (id_selector) @type
                (property_name) @keyword
                (string_value) @string
                (comment) @comment
            ",
            Self::Json => "
                (pair key: (string) @type)
                (string) @string
                (number) @number
                (true) @keyword
                (false) @keyword
                (null) @keyword
            ",
            Self::Bash => "
                (function_definition name: (word) @function)
                (string) @string
                (comment) @comment
                (variable_name) @type
                [
                  \"if\"
                  \"then\"
                  \"else\"
                  \"elif\"
                  \"fi\"
                  \"for\"
                  \"while\"
                  \"do\"
                  \"done\"
                  \"case\"
                  \"esac\"
                  \"function\"
                  \"in\"
                ] @keyword
            ",
            Self::Ruby => "
                (method name: (identifier) @function)
                (class name: (constant) @type)
                (constant) @type
                (string) @string
                (comment) @comment
                [
                  \"def\"
                  \"end\"
                  \"class\"
                  \"module\"
                  \"if\"
                  \"else\"
                  \"elsif\"
                  \"unless\"
                  \"while\"
                  \"until\"
                  \"for\"
                  \"do\"
                  \"return\"
                ] @keyword
                (nil) @keyword
                (true) @keyword
                (false) @keyword
                (self) @keyword
            ",
            Self::CSharp => "
                (method_declaration name: (identifier) @function)
                (constructor_declaration name: (identifier) @function)
                (local_function_statement name: (identifier) @function)
                (invocation_expression function: (identifier) @function)
                (class_declaration name: (identifier) @type)
                (interface_declaration name: (identifier) @type)
                (struct_declaration name: (identifier) @type)
                (enum_declaration name: (identifier) @type)
                (delegate_declaration name: (identifier) @type)
                (generic_name (identifier) @type)
                (void_keyword) @type
                (implicit_type) @keyword

                (variable_declaration type: (identifier) @type)
                (parameter type: (identifier) @type)
                (object_creation_expression type: (identifier) @type)
                (method_declaration type: (identifier) @type)
                (local_function_statement type: (identifier) @type)
                (property_declaration type: (identifier) @type)
                (event_declaration type: (identifier) @type)
                (cast_expression type: (identifier) @type)
                (for_each_statement type: (identifier) @type)
                (catch_declaration type: (identifier) @type)
                (base_list (identifier) @type)

                (using_directive (identifier) @type)
                (using_directive (qualified_name) @type)
                (namespace_declaration name: (identifier) @type)
                (namespace_declaration name: (qualified_name) @type)
                (file_scoped_namespace_declaration name: (identifier) @type)
                (file_scoped_namespace_declaration name: (qualified_name) @type)

                (string_literal) @string
                (verbatim_string_literal) @string
                (interpolated_string_expression) @string
                (character_literal) @string
                (integer_literal) @number
                (real_literal) @number
                (comment) @comment
                (member_access_expression name: (identifier) @variable)
                (attribute name: (identifier) @function)
                [
                  \"class\"
                  \"interface\"
                  \"struct\"
                  \"namespace\"
                  \"using\"
                  \"public\"
                  \"private\"
                  \"protected\"
                  \"internal\"
                  \"static\"
                  \"if\"
                  \"else\"
                  \"for\"
                  \"foreach\"
                  \"while\"
                  \"do\"
                  \"return\"
                  \"new\"
                  \"override\"
                  \"virtual\"
                  \"abstract\"
                  \"sealed\"
                  \"async\"
                  \"await\"
                  \"readonly\"
                  \"const\"
                  \"base\"
                  \"this\"
                  \"throw\"
                  \"try\"
                  \"catch\"
                  \"finally\"
                  \"switch\"
                  \"case\"
                  \"default\"
                  \"break\"
                  \"continue\"
                  \"enum\"
                  \"delegate\"
                  \"event\"
                  \"get\"
                  \"set\"
                  \"in\"
                  \"out\"
                  \"ref\"
                  \"params\"
                  \"is\"
                  \"as\"
                  \"typeof\"
                  \"partial\"
                ] @keyword
                (boolean_literal) @keyword
                (null_literal) @keyword
                (predefined_type) @type
            ",
            Self::Java => "
                (method_declaration name: (identifier) @function)
                (class_declaration name: (identifier) @type)
                (interface_declaration name: (identifier) @type)
                (string_literal) @string
                (line_comment) @comment
                (block_comment) @comment
                [
                  \"class\"
                  \"interface\"
                  \"extends\"
                  \"implements\"
                  \"public\"
                  \"private\"
                  \"protected\"
                  \"static\"
                  \"if\"
                  \"else\"
                  \"for\"
                  \"while\"
                  \"return\"
                  \"new\"
                  \"import\"
                  \"package\"
                ] @keyword
                (true) @keyword
                (false) @keyword
                (null_literal) @keyword
                (type_identifier) @type
            ",
            Self::Toml => "
                (bare_key) @type
                (quoted_key) @type
                (string) @string
                (integer) @number
                (float) @number
                (boolean) @keyword
                (comment) @comment
            ",
        }
    }
}

pub struct Syntax {
    parser: Parser,
    query: Query,
    #[allow(dead_code)] // Used in tests
    language: SyntaxLanguage,
    /// Most recently produced parse tree. Passed back to the parser on the next
    /// call to `parse()` so tree-sitter can skip re-parsing unchanged subtrees
    /// (incremental parsing). The tree is not explicitly edited with `InputEdit`
    /// before re-use — tree-sitter still benefits from unchanged subtree reuse
    /// as a best-effort optimisation.
    last_tree: Option<Tree>,
}

impl Syntax {
    /// Create a new Syntax highlighter for a specific language
    pub fn new_for_language(language: SyntaxLanguage) -> Self {
        let mut parser = Parser::new();
        let ts_language = language.language();
        parser
            .set_language(ts_language)
            .expect("Error loading grammar");

        let query_source = language.query_source();
        let query = Query::new(ts_language, query_source).expect("Error compiling query");

        Self {
            parser,
            query,
            language,
            last_tree: None,
        }
    }

    /// Create a new Syntax highlighter, detecting language from file path
    pub fn new_from_path(path: Option<&str>) -> Option<Self> {
        path.and_then(SyntaxLanguage::from_path)
            .map(Self::new_for_language)
    }

    /// Create a new Syntax highlighter for Rust (default/fallback)
    pub fn new() -> Self {
        Self::new_for_language(SyntaxLanguage::Rust)
    }
}

impl Default for Syntax {
    fn default() -> Self {
        Self::new()
    }
}

impl Syntax {
    #[allow(dead_code)] // Used in tests
    pub fn language(&self) -> SyntaxLanguage {
        self.language
    }

    pub fn parse(&mut self, text: &str) -> Vec<(usize, usize, String)> {
        // Pass the previous tree for incremental re-parsing.  Tree-sitter can
        // reuse unchanged subtrees even without explicit InputEdit calls, giving
        // a significant speedup on large files where only a small region changed.
        let tree = self
            .parser
            .parse(text, self.last_tree.as_ref())
            .expect("tree-sitter parse failed");

        let mut cursor = QueryCursor::new();
        let mut highlights = Vec::new();

        let matches = cursor.matches(&self.query, tree.root_node(), text.as_bytes());

        for m in matches {
            for capture in m.captures {
                let start = capture.node.start_byte();
                let end = capture.node.end_byte();
                let capture_name = self.query.capture_names()[capture.index as usize].clone();
                highlights.push((start, end, capture_name));
            }
        }

        self.last_tree = Some(tree);
        highlights
    }

    /// Walk the tree-sitter parse tree upward from the given cursor position
    /// and return the chain of enclosing scope-defining nodes (outermost first).
    pub fn enclosing_scopes(&self, text: &str, line: usize, col: usize) -> Vec<BreadcrumbSymbol> {
        let tree = match self.last_tree.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        let point = Point::new(line, col);
        let node = match tree.root_node().descendant_for_point_range(point, point) {
            Some(n) => n,
            None => return vec![],
        };

        let scope_kinds = Self::scope_kinds_for(self.language);
        if scope_kinds.is_empty() {
            return vec![];
        }

        let mut result = Vec::new();
        let mut cur = Some(node);
        while let Some(n) = cur {
            let kind = n.kind();
            if scope_kinds.contains(&kind) {
                let name = Self::extract_node_name(&n, text);
                if !name.is_empty() {
                    result.push(BreadcrumbSymbol {
                        name,
                        kind: kind.to_string(),
                    });
                }
            }
            cur = n.parent();
        }
        result.reverse();
        result
    }

    /// Return the set of tree-sitter node kinds that define scopes for breadcrumbs.
    fn scope_kinds_for(lang: SyntaxLanguage) -> &'static [&'static str] {
        match lang {
            SyntaxLanguage::Rust => &[
                "mod_item",
                "function_item",
                "impl_item",
                "struct_item",
                "enum_item",
                "trait_item",
            ],
            SyntaxLanguage::Python => &["class_definition", "function_definition"],
            SyntaxLanguage::JavaScript => &[
                "class_declaration",
                "function_declaration",
                "method_definition",
            ],
            SyntaxLanguage::TypeScript | SyntaxLanguage::TypeScriptReact => &[
                "class_declaration",
                "function_declaration",
                "method_definition",
                "interface_declaration",
            ],
            SyntaxLanguage::Go => &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
            ],
            SyntaxLanguage::Cpp => &[
                "function_definition",
                "struct_specifier",
                "class_specifier",
                "namespace_definition",
            ],
            SyntaxLanguage::C => &["function_definition", "struct_specifier"],
            SyntaxLanguage::Java => &[
                "class_declaration",
                "method_declaration",
                "interface_declaration",
            ],
            SyntaxLanguage::CSharp => &[
                "class_declaration",
                "method_declaration",
                "interface_declaration",
                "namespace_declaration",
            ],
            SyntaxLanguage::Ruby => &["class", "module", "method", "singleton_method"],
            _ => &[],
        }
    }

    /// Extract a human-readable name from a scope node.
    fn extract_node_name(node: &tree_sitter::Node, text: &str) -> String {
        // Try `name` field first (works for most node kinds)
        if let Some(name_node) = node.child_by_field_name("name") {
            return text[name_node.byte_range()].to_string();
        }
        // For impl_item: look for a type child (the type being implemented)
        if node.kind() == "impl_item" {
            if let Some(type_node) = node.child_by_field_name("type") {
                return text[type_node.byte_range()].to_string();
            }
        }
        // Fallback: scan children for an identifier-like node
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                let k = child.kind();
                if k == "identifier"
                    || k == "type_identifier"
                    || k == "field_identifier"
                    || k == "constant"
                    || k == "property_identifier"
                {
                    return text[child.byte_range()].to_string();
                }
            }
        }
        String::new()
    }
}

/// A symbol in the breadcrumb hierarchy (e.g. a function, struct, class).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbSymbol {
    pub name: String,
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection_rust() {
        assert_eq!(
            SyntaxLanguage::from_path("main.rs"),
            Some(SyntaxLanguage::Rust)
        );
        assert_eq!(
            SyntaxLanguage::from_path("/path/to/file.RS"),
            Some(SyntaxLanguage::Rust)
        );
    }

    #[test]
    fn test_language_detection_python() {
        assert_eq!(
            SyntaxLanguage::from_path("script.py"),
            Some(SyntaxLanguage::Python)
        );
        assert_eq!(
            SyntaxLanguage::from_path("app.pyw"),
            Some(SyntaxLanguage::Python)
        );
        assert_eq!(
            SyntaxLanguage::from_path("/path/to/file.PY"),
            Some(SyntaxLanguage::Python)
        );
    }

    #[test]
    fn test_language_detection_javascript() {
        assert_eq!(
            SyntaxLanguage::from_path("app.js"),
            Some(SyntaxLanguage::JavaScript)
        );
        assert_eq!(
            SyntaxLanguage::from_path("component.jsx"),
            Some(SyntaxLanguage::JavaScript)
        );
        assert_eq!(
            SyntaxLanguage::from_path("module.mjs"),
            Some(SyntaxLanguage::JavaScript)
        );
        assert_eq!(
            SyntaxLanguage::from_path("common.cjs"),
            Some(SyntaxLanguage::JavaScript)
        );
    }

    #[test]
    fn test_language_detection_go() {
        assert_eq!(
            SyntaxLanguage::from_path("main.go"),
            Some(SyntaxLanguage::Go)
        );
        assert_eq!(
            SyntaxLanguage::from_path("/path/to/file.GO"),
            Some(SyntaxLanguage::Go)
        );
    }

    #[test]
    fn test_language_detection_cpp() {
        assert_eq!(
            SyntaxLanguage::from_path("main.cpp"),
            Some(SyntaxLanguage::Cpp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("file.cc"),
            Some(SyntaxLanguage::Cpp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("file.cxx"),
            Some(SyntaxLanguage::Cpp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("file.c++"),
            Some(SyntaxLanguage::Cpp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("header.hpp"),
            Some(SyntaxLanguage::Cpp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("header.hh"),
            Some(SyntaxLanguage::Cpp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("header.hxx"),
            Some(SyntaxLanguage::Cpp)
        );
    }

    #[test]
    fn test_language_detection_c() {
        assert_eq!(SyntaxLanguage::from_path("main.c"), Some(SyntaxLanguage::C));
        assert_eq!(
            SyntaxLanguage::from_path("header.h"),
            Some(SyntaxLanguage::C)
        );
        assert_eq!(
            SyntaxLanguage::from_path("/path/to/FILE.C"),
            Some(SyntaxLanguage::C)
        );
    }

    #[test]
    fn test_language_detection_typescript() {
        assert_eq!(
            SyntaxLanguage::from_path("app.ts"),
            Some(SyntaxLanguage::TypeScript)
        );
        assert_eq!(
            SyntaxLanguage::from_path("component.tsx"),
            Some(SyntaxLanguage::TypeScriptReact)
        );
        assert_eq!(
            SyntaxLanguage::from_path("/path/to/file.TS"),
            Some(SyntaxLanguage::TypeScript)
        );
    }

    #[test]
    fn test_language_detection_html() {
        // HTML is not yet supported (tree-sitter-html 0.20.x depends on tree-sitter 0.22)
        assert_eq!(SyntaxLanguage::from_path("index.html"), None);
        assert_eq!(SyntaxLanguage::from_path("page.htm"), None);
    }

    #[test]
    fn test_language_detection_css() {
        assert_eq!(
            SyntaxLanguage::from_path("style.css"),
            Some(SyntaxLanguage::Css)
        );
        assert_eq!(
            SyntaxLanguage::from_path("STYLE.CSS"),
            Some(SyntaxLanguage::Css)
        );
    }

    #[test]
    fn test_language_detection_json() {
        assert_eq!(
            SyntaxLanguage::from_path("config.json"),
            Some(SyntaxLanguage::Json)
        );
        assert_eq!(
            SyntaxLanguage::from_path("tsconfig.jsonc"),
            Some(SyntaxLanguage::Json)
        );
    }

    #[test]
    fn test_language_detection_bash() {
        assert_eq!(
            SyntaxLanguage::from_path("build.sh"),
            Some(SyntaxLanguage::Bash)
        );
        assert_eq!(
            SyntaxLanguage::from_path("script.bash"),
            Some(SyntaxLanguage::Bash)
        );
        assert_eq!(
            SyntaxLanguage::from_path("config.zsh"),
            Some(SyntaxLanguage::Bash)
        );
    }

    #[test]
    fn test_language_detection_ruby() {
        assert_eq!(
            SyntaxLanguage::from_path("app.rb"),
            Some(SyntaxLanguage::Ruby)
        );
        assert_eq!(
            SyntaxLanguage::from_path("APP.RB"),
            Some(SyntaxLanguage::Ruby)
        );
    }

    #[test]
    fn test_language_detection_csharp() {
        assert_eq!(
            SyntaxLanguage::from_path("Program.cs"),
            Some(SyntaxLanguage::CSharp)
        );
        assert_eq!(
            SyntaxLanguage::from_path("PROGRAM.CS"),
            Some(SyntaxLanguage::CSharp)
        );
    }

    #[test]
    fn test_language_detection_java() {
        assert_eq!(
            SyntaxLanguage::from_path("Main.java"),
            Some(SyntaxLanguage::Java)
        );
        assert_eq!(
            SyntaxLanguage::from_path("MAIN.JAVA"),
            Some(SyntaxLanguage::Java)
        );
    }

    #[test]
    fn test_language_detection_toml() {
        assert_eq!(
            SyntaxLanguage::from_path("Cargo.toml"),
            Some(SyntaxLanguage::Toml)
        );
        assert_eq!(
            SyntaxLanguage::from_path("CARGO.TOML"),
            Some(SyntaxLanguage::Toml)
        );
    }

    #[test]
    fn test_language_detection_unknown() {
        assert_eq!(SyntaxLanguage::from_path("README.md"), None);
        assert_eq!(SyntaxLanguage::from_path("file.txt"), None);
        assert_eq!(SyntaxLanguage::from_path("no_extension"), None);
    }

    #[test]
    fn test_syntax_rust_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
        let code = "fn main() { let x = 42; }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_python_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Python);
        let code = "def hello():\n    print('world')";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_javascript_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::JavaScript);
        let code = "function hello() { return 'world'; }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_go_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Go);
        let code = "package main\nfunc main() {}";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_cpp_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Cpp);
        let code = "int main() { return 0; }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_c_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::C);
        let code = "int main() { return 0; }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_typescript_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::TypeScript);
        let code = "interface Foo { bar: string; }\nconst x: Foo = { bar: 'hello' };";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_css_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Css);
        let code = "body { color: red; }\n.foo { background: blue; }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_json_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Json);
        let code = r#"{"name": "test", "value": 42, "enabled": true}"#;
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_bash_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Bash);
        let code = "#!/bin/bash\n# comment\nif [ -f file ]; then\n  echo hello\nfi";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_ruby_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Ruby);
        let code = "def hello\n  puts 'world'\nend";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_csharp_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::CSharp);
        let code = "class Foo { public void Bar() { return; } }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_java_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Java);
        let code = "public class Main { public static void main(String[] args) {} }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_toml_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Toml);
        let code = "[package]\nname = \"vimcode\"\nversion = \"0.1.0\"";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
    }

    #[test]
    fn test_syntax_from_path() {
        let syntax_py = Syntax::new_from_path(Some("test.py"));
        assert!(syntax_py.is_some());
        assert_eq!(syntax_py.unwrap().language(), SyntaxLanguage::Python);

        let syntax_js = Syntax::new_from_path(Some("app.js"));
        assert!(syntax_js.is_some());
        assert_eq!(syntax_js.unwrap().language(), SyntaxLanguage::JavaScript);

        let syntax_ts = Syntax::new_from_path(Some("app.ts"));
        assert!(syntax_ts.is_some());
        assert_eq!(syntax_ts.unwrap().language(), SyntaxLanguage::TypeScript);

        let syntax_unknown = Syntax::new_from_path(Some("file.txt"));
        assert!(syntax_unknown.is_none());
    }

    #[test]
    fn test_enclosing_scopes_rust_fn() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
        let code = "fn hello() {\n    let x = 1;\n}\n";
        syntax.parse(code);
        let scopes = syntax.enclosing_scopes(code, 1, 8);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].name, "hello");
        assert_eq!(scopes[0].kind, "function_item");
    }

    #[test]
    fn test_enclosing_scopes_empty_without_parse() {
        let syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
        let scopes = syntax.enclosing_scopes("fn main() {}", 0, 5);
        assert!(scopes.is_empty());
    }

    #[test]
    fn test_enclosing_scopes_go() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Go);
        let code = "package main\nfunc hello() {\n\tx := 1\n}\n";
        syntax.parse(code);
        let scopes = syntax.enclosing_scopes(code, 2, 1);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].name, "hello");
    }
}
