use tree_sitter::{Language, Parser, Query, QueryCursor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxLanguage {
    Rust,
    Python,
    JavaScript,
    Go,
    Cpp,
}

impl SyntaxLanguage {
    /// Detect language from file extension
    pub fn from_path(path: &str) -> Option<Self> {
        let path_lower = path.to_lowercase();

        if path_lower.ends_with(".rs") {
            Some(Self::Rust)
        } else if path_lower.ends_with(".py") || path_lower.ends_with(".pyw") {
            Some(Self::Python)
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
            || path_lower.ends_with(".h")
            || path_lower.ends_with(".hh")
            || path_lower.ends_with(".hxx")
        {
            Some(Self::Cpp)
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
        }
    }
}

pub struct Syntax {
    parser: Parser,
    query: Query,
    #[allow(dead_code)] // Used in tests
    language: SyntaxLanguage,
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

    #[allow(dead_code)] // Used in tests
    pub fn language(&self) -> SyntaxLanguage {
        self.language
    }

    pub fn parse(&mut self, text: &str) -> Vec<(usize, usize, String)> {
        let tree = self.parser.parse(text, None).unwrap();
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
        highlights
    }
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
            SyntaxLanguage::from_path("header.h"),
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
    fn test_syntax_from_path() {
        let syntax_py = Syntax::new_from_path(Some("test.py"));
        assert!(syntax_py.is_some());
        assert_eq!(syntax_py.unwrap().language(), SyntaxLanguage::Python);

        let syntax_js = Syntax::new_from_path(Some("app.js"));
        assert!(syntax_js.is_some());
        assert_eq!(syntax_js.unwrap().language(), SyntaxLanguage::JavaScript);

        let syntax_unknown = Syntax::new_from_path(Some("file.txt"));
        assert!(syntax_unknown.is_none());
    }
}
