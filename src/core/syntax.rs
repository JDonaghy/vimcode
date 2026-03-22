use streaming_iterator::StreamingIterator;
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
    Html,
    Css,
    Json,
    Bash,
    Ruby,
    CSharp,
    Java,
    Toml,
    Yaml,
    Latex,
    Lua,
    Markdown,
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
        } else if path_lower.ends_with(".yaml") || path_lower.ends_with(".yml") {
            Some(Self::Yaml)
        } else if path_lower.ends_with(".html") || path_lower.ends_with(".htm") {
            Some(Self::Html)
        } else if path_lower.ends_with(".tex")
            || path_lower.ends_with(".bib")
            || path_lower.ends_with(".cls")
            || path_lower.ends_with(".sty")
            || path_lower.ends_with(".dtx")
            || path_lower.ends_with(".ltx")
        {
            Some(Self::Latex)
        } else if path_lower.ends_with(".lua") {
            Some(Self::Lua)
        } else if path_lower.ends_with(".md")
            || path_lower.ends_with(".markdown")
            || path_lower.ends_with(".mdx")
        {
            Some(Self::Markdown)
        } else {
            None
        }
    }

    /// Map a markdown fence language tag (e.g. "rust", "python", "js") to a
    /// `SyntaxLanguage`.  Used for syntax-highlighting code blocks in hover
    /// popups and markdown previews.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "python" | "py" => Some(Self::Python),
            "javascript" | "js" | "jsx" => Some(Self::JavaScript),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::TypeScriptReact),
            "go" | "golang" => Some(Self::Go),
            "c" => Some(Self::C),
            "cpp" | "c++" | "cxx" | "cc" => Some(Self::Cpp),
            "csharp" | "c#" | "cs" => Some(Self::CSharp),
            "java" => Some(Self::Java),
            "ruby" | "rb" => Some(Self::Ruby),
            "bash" | "sh" | "shell" | "zsh" => Some(Self::Bash),
            "json" | "jsonc" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "html" | "htm" => Some(Self::Html),
            "css" => Some(Self::Css),
            "lua" => Some(Self::Lua),
            "latex" | "tex" => Some(Self::Latex),
            "markdown" | "md" => Some(Self::Markdown),
            _ => None,
        }
    }

    fn language(&self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::TypeScriptReact => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Html => tree_sitter_html::LANGUAGE.into(),
            Self::Css => tree_sitter_css::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            Self::Bash => tree_sitter_bash::LANGUAGE.into(),
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Self::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Self::Latex => {
                #[link(name = "tree_sitter_latex")]
                extern "C" {
                    fn tree_sitter_latex() -> *const ();
                }
                let lang_fn =
                    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_latex) };
                lang_fn.into()
            }
            Self::Lua => tree_sitter_lua::LANGUAGE.into(),
            Self::Markdown => tree_sitter_md::LANGUAGE.into(),
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
                (implicit_type) @keyword

                (variable_declaration type: (identifier) @type)
                (parameter type: (identifier) @type)
                (object_creation_expression type: (identifier) @type)
                (property_declaration type: (identifier) @type)
                (event_declaration type: (identifier) @type)
                (cast_expression type: (identifier) @type)
                (foreach_statement type: (identifier) @type)
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
            Self::Yaml => "
                (block_mapping_pair key: (flow_node) @type)
                (flow_mapping (_ key: (flow_node) @type))
                (double_quote_scalar) @string
                (single_quote_scalar) @string
                (block_scalar) @string
                (integer_scalar) @number
                (float_scalar) @number
                (boolean_scalar) @keyword
                (null_scalar) @keyword
                (comment) @comment
                (anchor_name) @function
                (alias_name) @function
                (tag) @function
            ",
            Self::Html => "
                (tag_name) @keyword
                (attribute_name) @type
                (attribute_value) @string
                (quoted_attribute_value) @string
                (comment) @comment
                (doctype) @keyword
                (raw_text) @string
            ",
            Self::Latex => "
                (line_comment) @comment
                (block_comment) @comment
                (generic_command command: (command_name) @keyword)
                (section) @function
                (subsection) @function
                (subsubsection) @function
                (chapter) @function
                (paragraph) @function
                (begin) @keyword
                (end) @keyword
                (class_include) @keyword
                (package_include) @keyword
                (new_command_definition) @keyword
                (label_definition) @variable
                (label_reference) @variable
                (citation) @string
                (inline_formula) @type
                (math_environment) @type
                (displayed_equation) @type
            ",
            Self::Lua => "
                (function_declaration name: (identifier) @function)
                (function_call name: (identifier) @function)
                (string) @string
                (comment) @comment
                (number) @number
                [
                  \"function\" \"end\" \"local\" \"return\" \"if\" \"then\" \"else\" \"elseif\"
                  \"for\" \"while\" \"do\" \"repeat\" \"until\" \"in\" \"not\"
                  \"and\" \"or\"
                ] @keyword
                (break_statement) @keyword
                (goto_statement) @keyword
                (true) @keyword
                (false) @keyword
                (nil) @keyword
            ",
            Self::Markdown => "
                (atx_heading) @function
                (setext_heading) @function
                (fenced_code_block) @string
                (indented_code_block) @string
                (thematic_break) @comment
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
            .set_language(&ts_language)
            .expect("Error loading grammar");

        let query_source = language.query_source();
        let query = Query::new(&ts_language, query_source).expect("Error compiling query");

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
}

impl Syntax {
    #[allow(dead_code)] // Used in tests
    pub fn language(&self) -> SyntaxLanguage {
        self.language
    }

    pub fn parse(&mut self, text: &str) -> Vec<(usize, usize, String)> {
        self.reparse(text);
        self.extract_highlights(text)
    }

    /// Incrementally re-parse the text without extracting highlights.
    /// This is fast (tree-sitter reuses unchanged subtrees) and should be
    /// called on every keystroke. Highlight extraction can be deferred.
    pub fn reparse(&mut self, text: &str) {
        // Skip incremental parsing for Markdown: tree-sitter-md's external
        // scanner can corrupt the parser's logger struct when reusing an old
        // tree, causing a SIGSEGV in ts_parser__log.
        let old_tree = if self.language == SyntaxLanguage::Markdown {
            None
        } else {
            self.last_tree.as_ref()
        };
        let tree = self
            .parser
            .parse(text, old_tree)
            .expect("tree-sitter parse failed");
        self.last_tree = Some(tree);
    }

    /// Extract highlights from the most recent parse tree.
    /// This is the expensive part — O(number of captures in the file).
    pub fn extract_highlights(&self, text: &str) -> Vec<(usize, usize, String)> {
        let tree = match &self.last_tree {
            Some(t) => t,
            None => return Vec::new(),
        };
        let mut cursor = QueryCursor::new();
        let mut highlights = Vec::new();

        let mut matches = cursor.matches(&self.query, tree.root_node(), text.as_bytes());

        while let Some(m) = matches.next() {
            for capture in m.captures {
                let start = capture.node.start_byte();
                let end = capture.node.end_byte();
                let capture_name = self.query.capture_names()[capture.index as usize].to_string();
                highlights.push((start, end, capture_name));
            }
        }

        highlights
    }

    /// Extract highlights only for a byte range (e.g. visible viewport).
    /// Much faster than full extraction for large files.
    pub fn extract_highlights_range(
        &self,
        text: &str,
        start_byte: usize,
        end_byte: usize,
    ) -> Vec<(usize, usize, String)> {
        let tree = match &self.last_tree {
            Some(t) => t,
            None => return Vec::new(),
        };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start_byte..end_byte);
        let mut highlights = Vec::new();

        let mut matches = cursor.matches(&self.query, tree.root_node(), text.as_bytes());

        while let Some(m) = matches.next() {
            for capture in m.captures {
                let start = capture.node.start_byte();
                let end = capture.node.end_byte();
                let capture_name = self.query.capture_names()[capture.index as usize].to_string();
                highlights.push((start, end, capture_name));
            }
        }

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
            SyntaxLanguage::Latex => &[
                "generic_environment",
                "section",
                "chapter",
                "subsection",
                "subsubsection",
            ],
            SyntaxLanguage::Lua => &["function_declaration", "function_definition"],
            SyntaxLanguage::Markdown => &["atx_heading", "setext_heading", "section"],
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
            if let Some(child) = node.child(i as u32) {
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
        assert_eq!(
            SyntaxLanguage::from_path("index.html"),
            Some(SyntaxLanguage::Html)
        );
        assert_eq!(
            SyntaxLanguage::from_path("page.htm"),
            Some(SyntaxLanguage::Html)
        );
    }

    #[test]
    fn test_language_detection_yaml() {
        assert_eq!(
            SyntaxLanguage::from_path("config.yaml"),
            Some(SyntaxLanguage::Yaml)
        );
        assert_eq!(
            SyntaxLanguage::from_path("config.yml"),
            Some(SyntaxLanguage::Yaml)
        );
        assert_eq!(
            SyntaxLanguage::from_path("CONFIG.YAML"),
            Some(SyntaxLanguage::Yaml)
        );
    }

    #[test]
    fn test_language_detection_latex() {
        assert_eq!(
            SyntaxLanguage::from_path("paper.tex"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(
            SyntaxLanguage::from_path("refs.bib"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(
            SyntaxLanguage::from_path("custom.cls"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(
            SyntaxLanguage::from_path("package.sty"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(
            SyntaxLanguage::from_path("doc.dtx"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(
            SyntaxLanguage::from_path("main.ltx"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(
            SyntaxLanguage::from_path("PAPER.TEX"),
            Some(SyntaxLanguage::Latex)
        );
    }

    #[test]
    fn test_language_detection_lua() {
        assert_eq!(
            SyntaxLanguage::from_path("init.lua"),
            Some(SyntaxLanguage::Lua)
        );
    }

    #[test]
    fn test_language_detection_markdown() {
        assert_eq!(
            SyntaxLanguage::from_path("README.md"),
            Some(SyntaxLanguage::Markdown)
        );
        assert_eq!(
            SyntaxLanguage::from_path("notes.markdown"),
            Some(SyntaxLanguage::Markdown)
        );
        assert_eq!(
            SyntaxLanguage::from_path("page.mdx"),
            Some(SyntaxLanguage::Markdown)
        );
    }

    #[test]
    fn test_lua_parse_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Lua);
        let highlights = syntax.parse("function hello()\n  local x = 42\nend\n");
        assert!(!highlights.is_empty());
        // Should have keyword and function captures
        assert!(highlights.iter().any(|(_, _, s)| s == "keyword"));
        assert!(highlights.iter().any(|(_, _, s)| s == "function"));
    }

    #[test]
    fn test_markdown_parse_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Markdown);
        let highlights = syntax.parse("# Hello World\n\nSome text.\n\n```rust\nlet x = 1;\n```\n");
        assert!(!highlights.is_empty());
        // Should have heading (function) captures
        assert!(highlights.iter().any(|(_, _, s)| s == "function"));
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
        assert_eq!(SyntaxLanguage::from_path("file.txt"), None);
        assert_eq!(SyntaxLanguage::from_path("no_extension"), None);
    }

    #[test]
    fn test_from_name() {
        assert_eq!(
            SyntaxLanguage::from_name("rust"),
            Some(SyntaxLanguage::Rust)
        );
        assert_eq!(SyntaxLanguage::from_name("rs"), Some(SyntaxLanguage::Rust));
        assert_eq!(
            SyntaxLanguage::from_name("Rust"),
            Some(SyntaxLanguage::Rust)
        );
        assert_eq!(
            SyntaxLanguage::from_name("python"),
            Some(SyntaxLanguage::Python)
        );
        assert_eq!(
            SyntaxLanguage::from_name("py"),
            Some(SyntaxLanguage::Python)
        );
        assert_eq!(
            SyntaxLanguage::from_name("js"),
            Some(SyntaxLanguage::JavaScript)
        );
        assert_eq!(
            SyntaxLanguage::from_name("typescript"),
            Some(SyntaxLanguage::TypeScript)
        );
        assert_eq!(
            SyntaxLanguage::from_name("tsx"),
            Some(SyntaxLanguage::TypeScriptReact)
        );
        assert_eq!(
            SyntaxLanguage::from_name("golang"),
            Some(SyntaxLanguage::Go)
        );
        assert_eq!(SyntaxLanguage::from_name("c++"), Some(SyntaxLanguage::Cpp));
        assert_eq!(
            SyntaxLanguage::from_name("c#"),
            Some(SyntaxLanguage::CSharp)
        );
        assert_eq!(
            SyntaxLanguage::from_name("shell"),
            Some(SyntaxLanguage::Bash)
        );
        assert_eq!(SyntaxLanguage::from_name("yml"), Some(SyntaxLanguage::Yaml));
        assert_eq!(
            SyntaxLanguage::from_name("tex"),
            Some(SyntaxLanguage::Latex)
        );
        assert_eq!(SyntaxLanguage::from_name("unknown_lang"), None);
        assert_eq!(SyntaxLanguage::from_name(""), None);
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
    fn test_syntax_yaml_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Yaml);
        let code = "# comment\napp_name: \"MyApp\"\nversion: 1.0\nenabled: true\n";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
        let kinds: Vec<&str> = highlights.iter().map(|(_, _, k)| k.as_str()).collect();
        assert!(kinds.contains(&"comment"), "should highlight comments");
        assert!(kinds.contains(&"string"), "should highlight quoted strings");
        assert!(kinds.contains(&"type"), "should highlight keys as type");
        assert!(kinds.contains(&"number"), "should highlight numbers");
        assert!(
            kinds.contains(&"keyword"),
            "should highlight booleans as keyword"
        );
        // Keys must not be overridden by string — check that key byte range has type, not string
        let key_highlights: Vec<_> = highlights
            .iter()
            .filter(|(s, e, _)| *s == 10 && *e == 18)
            .collect();
        assert!(
            key_highlights.iter().any(|(_, _, k)| k == "type"),
            "key should be type"
        );
        assert!(
            !key_highlights.iter().any(|(_, _, k)| k == "string"),
            "key should NOT be string"
        );
    }

    #[test]
    fn test_syntax_latex_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Latex);
        let code = "\\documentclass{article}\n\\begin{document}\nHello world.\n% a comment\n$x^2 + y^2$\n\\end{document}\n";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
        let kinds: Vec<&str> = highlights.iter().map(|(_, _, k)| k.as_str()).collect();
        assert!(kinds.contains(&"comment"), "should highlight comments");
        assert!(kinds.contains(&"keyword"), "should highlight commands");
        assert!(kinds.contains(&"type"), "should highlight math");
    }

    #[test]
    fn test_syntax_html_basic() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Html);
        let code = "<html><body class=\"main\">Hello</body></html>";
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
