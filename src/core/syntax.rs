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
    /// Map an LSP language identifier (e.g. "rust", "python") to a SyntaxLanguage.
    pub fn from_language_id(id: &str) -> Option<Self> {
        match id {
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            "javascript" | "javascriptreact" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "typescriptreact" => Some(Self::TypeScriptReact),
            "go" => Some(Self::Go),
            "c" => Some(Self::C),
            "cpp" => Some(Self::Cpp),
            "csharp" => Some(Self::CSharp),
            "java" => Some(Self::Java),
            "ruby" => Some(Self::Ruby),
            "lua" => Some(Self::Lua),
            "shellscript" => Some(Self::Bash),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" => Some(Self::Yaml),
            "html" => Some(Self::Html),
            "css" => Some(Self::Css),
            "markdown" => Some(Self::Markdown),
            "latex" | "bibtex" => Some(Self::Latex),
            _ => None,
        }
    }

    /// Return the LSP language ID for this language.
    pub fn language_id(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::TypeScriptReact => "typescriptreact",
            Self::Go => "go",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Java => "java",
            Self::Ruby => "ruby",
            Self::Lua => "lua",
            Self::Bash => "shellscript",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Html => "html",
            Self::Css => "css",
            Self::Markdown => "markdown",
            Self::Latex => "latex",
        }
    }

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
                #[link(name = "tree_sitter_latex", kind = "static")]
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
                (call_expression function: (identifier) @function.call)
                (call_expression function: (field_expression field: (field_identifier) @method.call))
                (call_expression function: (scoped_identifier name: (identifier) @function.call))
                (macro_invocation macro: (identifier) @macro)
                (macro_invocation macro: (scoped_identifier name: (identifier) @macro))
                (macro_definition name: (identifier) @macro)
                (type_identifier) @type
                (primitive_type) @type
                (scoped_type_identifier name: (type_identifier) @type)
                (string_literal) @string
                (raw_string_literal) @string
                (char_literal) @string
                (integer_literal) @number
                (float_literal) @number
                (boolean_literal) @boolean
                (line_comment) @comment
                (block_comment) @comment
                (attribute_item) @attribute
                (inner_attribute_item) @attribute
                (lifetime (identifier) @lifetime)
                (mod_item name: (identifier) @module)
                (scoped_identifier path: (identifier) @module)
                (field_expression field: (field_identifier) @property)
                (field_declaration name: (field_identifier) @property)
                (shorthand_field_initializer (identifier) @property)
                (parameter pattern: (identifier) @parameter)
                (self) @variable
                (mutable_specifier) @keyword
                (escape_sequence) @escape
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\";\" \",\" \"::\" \".\"] @punctuation.delimiter
                [\"=\" \"+=\" \"-=\" \"*=\" \"/=\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"&\" \"|\" \"^\" \"+\" \"-\" \"*\" \"/\" \"%\" \"..\" \"?\" ] @operator
                [\"->\" \"=>\"] @operator
                [
                  \"fn\" \"struct\" \"enum\" \"impl\" \"pub\" \"use\" \"mod\" \"let\"
                  \"const\" \"static\" \"trait\" \"where\" \"type\" \"as\" \"dyn\"
                  \"async\" \"await\" \"move\" \"ref\" \"unsafe\" \"extern\"
                ] @keyword
                [
                  \"if\" \"else\" \"match\" \"for\" \"while\" \"loop\" \"return\"
                  \"in\" \"break\" \"continue\" \"yield\"
                ] @keyword.control
            ",
            Self::Python => "
                (function_definition name: (identifier) @function)
                (class_definition name: (identifier) @type)
                (call function: (identifier) @function.call)
                (call function: (attribute attribute: (identifier) @method.call))
                (string) @string
                (integer) @number
                (float) @number
                (true) @boolean
                (false) @boolean
                (none) @constant
                (comment) @comment
                (decorator) @attribute
                (escape_sequence) @escape
                (attribute attribute: (identifier) @property)
                (parameters (identifier) @parameter)
                (default_parameter name: (identifier) @parameter)
                (typed_parameter (identifier) @parameter)
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \":\" \";\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"//\" \"%\" \"**\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"+=\" \"-=\" \"*=\" \"/=\"] @operator
                [\"and\" \"or\" \"not\" \"in\" \"is\"] @operator
                [\"def\" \"class\" \"import\" \"from\" \"as\" \"with\" \"lambda\"
                  \"async\" \"await\" \"global\" \"nonlocal\" \"del\" \"assert\"
                ] @keyword
                [\"if\" \"elif\" \"else\" \"for\" \"while\" \"return\" \"try\" \"except\"
                  \"finally\" \"pass\" \"break\" \"continue\" \"raise\" \"yield\"
                ] @keyword.control
            ",
            Self::JavaScript => "
                (function_declaration name: (identifier) @function)
                (method_definition name: (property_identifier) @function)
                (call_expression function: (identifier) @function.call)
                (call_expression function: (member_expression property: (property_identifier) @method.call))
                (class_declaration name: (identifier) @type)
                (string) @string
                (template_string) @string
                (number) @number
                (true) @boolean
                (false) @boolean
                (null) @constant
                (comment) @comment
                (regex) @string
                (member_expression property: (property_identifier) @property)
                (pair key: (property_identifier) @property)
                (shorthand_property_identifier) @property
                (formal_parameters (identifier) @parameter)
                (escape_sequence) @escape
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"===\" \"!=\" \"!==\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"+=\" \"-=\" \"*=\" \"/=\"] @operator
                [\"=>\" \"...\" \"??\" \"instanceof\" \"typeof\"] @operator
                (this) @variable
                [\"function\" \"class\" \"const\" \"let\" \"var\" \"new\" \"async\" \"await\"
                  \"import\" \"export\" \"from\" \"default\" \"void\" \"delete\" \"of\" \"in\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"while\" \"do\" \"return\" \"try\" \"catch\" \"finally\"
                  \"throw\" \"break\" \"continue\" \"switch\" \"case\" \"yield\"
                ] @keyword.control
            ",
            Self::Go => "
                (function_declaration name: (identifier) @function)
                (method_declaration name: (field_identifier) @function)
                (call_expression function: (identifier) @function.call)
                (call_expression function: (selector_expression field: (field_identifier) @method.call))
                (type_declaration (type_spec name: (type_identifier) @type))
                (type_identifier) @type
                (interpreted_string_literal) @string
                (raw_string_literal) @string
                (rune_literal) @string
                (int_literal) @number
                (float_literal) @number
                (imaginary_literal) @number
                (true) @boolean
                (false) @boolean
                (nil) @constant
                (comment) @comment
                (selector_expression field: (field_identifier) @property)
                (field_declaration name: (field_identifier) @property)
                (package_identifier) @module
                (parameter_declaration name: (identifier) @parameter)
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"&\" \"|\" \"^\" \":=\" \"+=\" \"-=\" \"<-\"] @operator
                [\"func\" \"package\" \"import\" \"type\" \"struct\" \"interface\"
                  \"go\" \"defer\" \"var\" \"const\" \"chan\" \"map\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"range\" \"return\" \"switch\" \"case\" \"default\"
                  \"break\" \"continue\" \"fallthrough\" \"select\"
                ] @keyword.control
            ",
            Self::Cpp => "
                (function_definition declarator: (function_declarator declarator: (identifier) @function))
                (declaration declarator: (function_declarator declarator: (identifier) @function))
                (call_expression function: (identifier) @function.call)
                (call_expression function: (field_expression field: (field_identifier) @method.call))
                (class_specifier name: (type_identifier) @type)
                (struct_specifier name: (type_identifier) @type)
                (type_identifier) @type
                (primitive_type) @type
                (namespace_identifier) @module
                (string_literal) @string
                (char_literal) @string
                (raw_string_literal) @string
                (number_literal) @number
                (true) @boolean
                (false) @boolean
                (null) @constant
                (comment) @comment
                (field_expression field: (field_identifier) @property)
                (field_declaration declarator: (field_identifier) @property)
                (parameter_declaration declarator: (identifier) @parameter)
                (preproc_include) @macro
                (preproc_def name: (identifier) @macro)
                (preproc_function_def name: (identifier) @macro)
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\" \"::\" \"->\" ] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"&\" \"|\" \"^\" \"+=\" \"-=\"] @operator
                [\"++\" \"--\"] @operator
                [\"class\" \"struct\" \"enum\" \"namespace\" \"public\" \"private\" \"protected\"
                  \"virtual\" \"static\" \"const\" \"template\" \"typename\" \"using\" \"new\" \"delete\"
                  \"constexpr\" \"noexcept\" \"override\" \"final\" \"explicit\"
                  \"inline\" \"volatile\" \"extern\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"while\" \"do\" \"return\" \"break\" \"continue\"
                  \"switch\" \"case\" \"default\" \"try\" \"catch\" \"throw\"
                ] @keyword.control
            ",
            Self::C => "
                (function_definition declarator: (function_declarator declarator: (identifier) @function))
                (declaration declarator: (function_declarator declarator: (identifier) @function))
                (call_expression function: (identifier) @function.call)
                (struct_specifier name: (type_identifier) @type)
                (enum_specifier name: (type_identifier) @type)
                (type_identifier) @type
                (primitive_type) @type
                (string_literal) @string
                (char_literal) @string
                (number_literal) @number
                (true) @boolean
                (false) @boolean
                (null) @constant
                (comment) @comment
                (field_expression field: (field_identifier) @property)
                (field_declaration declarator: (field_identifier) @property)
                (parameter_declaration declarator: (identifier) @parameter)
                (preproc_include) @macro
                (preproc_def name: (identifier) @macro)
                (preproc_function_def name: (identifier) @macro)
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"&\" \"|\" \"^\" \"+=\" \"-=\"] @operator
                [\"->\" \"++\" \"--\"] @operator
                [\"struct\" \"enum\" \"typedef\" \"static\" \"const\" \"sizeof\"
                  \"extern\" \"inline\" \"volatile\" \"unsigned\" \"signed\" \"union\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"while\" \"do\" \"return\" \"break\" \"continue\"
                  \"switch\" \"case\" \"default\" \"goto\"
                ] @keyword.control
            ",
            Self::TypeScript | Self::TypeScriptReact => "
                (function_declaration name: (identifier) @function)
                (method_definition name: (property_identifier) @function)
                (call_expression function: (identifier) @function.call)
                (call_expression function: (member_expression property: (property_identifier) @method.call))
                (class_declaration name: (type_identifier) @type)
                (interface_declaration name: (type_identifier) @type)
                (type_alias_declaration name: (type_identifier) @type)
                (type_identifier) @type
                (string) @string
                (template_string) @string
                (number) @number
                (comment) @comment
                (member_expression property: (property_identifier) @property)
                (pair key: (property_identifier) @property)
                (shorthand_property_identifier) @property
                (required_parameter pattern: (identifier) @parameter)
                (optional_parameter pattern: (identifier) @parameter)
                (escape_sequence) @escape
                (this) @variable
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"===\" \"!=\" \"!==\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"+=\" \"-=\" \"*=\" \"/=\"] @operator
                [\"=>\" \"...\" \"??\" \"instanceof\" \"typeof\"] @operator
                [\"function\" \"class\" \"interface\" \"type\" \"const\" \"let\" \"var\"
                  \"new\" \"async\" \"await\" \"import\" \"export\" \"from\" \"default\"
                  \"as\" \"extends\" \"implements\" \"void\" \"delete\" \"of\" \"in\"
                  \"declare\" \"enum\" \"namespace\" \"readonly\" \"abstract\" \"override\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"while\" \"do\" \"return\" \"try\" \"catch\" \"finally\"
                  \"throw\" \"break\" \"continue\" \"switch\" \"case\" \"yield\"
                ] @keyword.control
            ",
            Self::Css => "
                (tag_name) @function
                (class_selector) @type
                (id_selector) @type
                (property_name) @property
                (string_value) @string
                (color_value) @number
                (integer_value) @number
                (float_value) @number
                (plain_value) @variable
                (comment) @comment
                [\"{\" \"}\" \"(\" \")\" \"[\" \"]\"] @punctuation.bracket
                [\";\" \":\" \",\"] @punctuation.delimiter
                (important) @keyword
            ",
            Self::Json => "
                (pair key: (string) @property)
                (string) @string
                (number) @number
                (true) @boolean
                (false) @boolean
                (null) @constant
                [\"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \":\"] @punctuation.delimiter
            ",
            Self::Bash => "
                (function_definition name: (word) @function)
                (command_name (word) @function.call)
                (string) @string
                (raw_string) @string
                (number) @number
                (comment) @comment
                (variable_name) @variable
                (simple_expansion) @variable
                (expansion) @variable
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\" \"((\" \"))\" \"[[\" \"]]\"] @punctuation.bracket
                [\";\" \";;\" \"|\" \"||\" \"&&\" \"&\"] @punctuation.delimiter
                [\"=\" \"==\" \"!=\"] @operator
                [\">\" \">>\" \"<\" \"<<\"] @operator
                [\"function\" \"local\" \"export\" \"unset\" \"declare\"] @keyword
                [\"if\" \"then\" \"else\" \"elif\" \"fi\" \"for\" \"while\" \"do\" \"done\"
                  \"case\" \"esac\" \"in\" \"select\" \"until\"
                ] @keyword.control
            ",
            Self::Ruby => "
                (method name: (identifier) @function)
                (call method: (identifier) @method.call)
                (class name: (constant) @type)
                (constant) @type
                (string) @string
                (integer) @number
                (float) @number
                (nil) @constant
                (true) @boolean
                (false) @boolean
                (self) @variable
                (comment) @comment
                (simple_symbol) @string
                (escape_sequence) @escape
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\"] @operator
                [\"and\" \"or\" \"not\"] @operator
                [\"def\" \"end\" \"class\" \"module\"] @keyword
                [\"if\" \"else\" \"elsif\" \"unless\" \"while\" \"until\" \"for\" \"do\"
                  \"return\" \"begin\" \"rescue\" \"ensure\" \"yield\" \"break\" \"next\"
                ] @keyword.control
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
                [\"class\" \"interface\" \"struct\" \"namespace\" \"using\"
                  \"public\" \"private\" \"protected\" \"internal\" \"static\"
                  \"new\" \"override\" \"virtual\" \"abstract\" \"sealed\"
                  \"async\" \"await\" \"readonly\" \"const\" \"base\" \"this\"
                  \"enum\" \"delegate\" \"event\" \"get\" \"set\"
                  \"in\" \"out\" \"ref\" \"params\" \"is\" \"as\" \"typeof\" \"partial\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"foreach\" \"while\" \"do\" \"return\"
                  \"throw\" \"try\" \"catch\" \"finally\" \"switch\" \"case\" \"default\"
                  \"break\" \"continue\"
                ] @keyword.control
                (boolean_literal) @boolean
                (null_literal) @constant
                (predefined_type) @type
                (escape_sequence) @escape
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"&\" \"|\" \"^\" \"+=\" \"-=\" \"*=\" \"/=\"] @operator
                [\"++\" \"--\"] @operator
            ",
            Self::Java => "
                (method_declaration name: (identifier) @function)
                (method_invocation name: (identifier) @method.call)
                (class_declaration name: (identifier) @type)
                (interface_declaration name: (identifier) @type)
                (type_identifier) @type
                (string_literal) @string
                (character_literal) @string
                (decimal_integer_literal) @number
                (hex_integer_literal) @number
                (decimal_floating_point_literal) @number
                (true) @boolean
                (false) @boolean
                (null_literal) @constant
                (line_comment) @comment
                (block_comment) @comment
                (field_access field: (identifier) @property)
                (formal_parameter name: (identifier) @parameter)
                (marker_annotation name: (identifier) @attribute)
                (annotation name: (identifier) @attribute)
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"!=\" \"<\" \">\" \"<=\" \">=\" \"&&\" \"||\" \"!\" \"&\" \"|\" \"^\" \"+=\" \"-=\" \"*=\" \"/=\"] @operator
                [\"++\" \"--\" \"instanceof\"] @operator
                [\"class\" \"interface\" \"extends\" \"implements\" \"public\" \"private\"
                  \"protected\" \"static\" \"new\" \"import\" \"package\"
                  \"abstract\" \"final\" \"synchronized\" \"enum\"
                ] @keyword
                [\"if\" \"else\" \"for\" \"while\" \"return\" \"throw\" \"throws\"
                  \"try\" \"catch\" \"finally\" \"break\" \"continue\" \"do\"
                  \"switch\" \"case\" \"default\"
                ] @keyword.control
            ",
            Self::Toml => "
                (bare_key) @property
                (quoted_key) @property
                (string) @string
                (integer) @number
                (float) @number
                (boolean) @boolean
                (comment) @comment
                [\"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\"=\" \",\" \".\"] @punctuation.delimiter
            ",
            Self::Yaml => "
                (block_mapping_pair key: (flow_node) @property)
                (flow_mapping (_ key: (flow_node) @property))
                (double_quote_scalar) @string
                (single_quote_scalar) @string
                (block_scalar) @string
                (integer_scalar) @number
                (float_scalar) @number
                (boolean_scalar) @boolean
                (null_scalar) @constant
                (comment) @comment
                (anchor_name) @function
                (alias_name) @function
                (tag) @attribute
            ",
            Self::Html => "
                (tag_name) @keyword
                (attribute_name) @attribute
                (attribute_value) @string
                (quoted_attribute_value) @string
                (comment) @comment
                (doctype) @keyword
                (raw_text) @string
                [\"<\" \">\" \"</\" \"/>\" ] @punctuation.bracket
                [\"=\"] @operator
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
                (function_call name: (identifier) @function.call)
                (function_call name: (dot_index_expression field: (identifier) @method.call))
                (string) @string
                (comment) @comment
                (number) @number
                (true) @boolean
                (false) @boolean
                (nil) @constant
                (dot_index_expression field: (identifier) @property)
                (break_statement) @keyword
                (goto_statement) @keyword
                [\"(\" \")\" \"[\" \"]\" \"{\" \"}\"] @punctuation.bracket
                [\",\" \".\" \";\" \":\"] @punctuation.delimiter
                [\"=\" \"+\" \"-\" \"*\" \"/\" \"%\" \"==\" \"~=\" \"<\" \">\" \"<=\" \">=\" \"..\" \"#\"] @operator
                [\"and\" \"or\" \"not\"] @operator
                [\"function\" \"end\" \"local\"] @keyword
                [\"return\" \"if\" \"then\" \"else\" \"elseif\"
                  \"for\" \"while\" \"do\" \"repeat\" \"until\" \"in\"
                ] @keyword.control
            ",
            Self::Markdown => "
                (atx_heading) @function
                (setext_heading) @function
                (fenced_code_block) @string
                (indented_code_block) @string
                (thematic_break) @comment
                (block_quote_marker) @comment
                (block_quote (paragraph) @comment)
                (list_marker_minus) @keyword
                (list_marker_plus) @keyword
                (list_marker_star) @keyword
                (list_marker_dot) @keyword
                (list_marker_parenthesis) @keyword
                (fenced_code_block_delimiter) @comment
                (info_string) @type
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
        Self::new_for_language_with_query(language, None)
    }

    /// Create a Syntax highlighter, optionally using a custom highlight query.
    /// Falls back to the built-in query if `override_query` is None or fails to compile.
    pub fn new_for_language_with_query(
        language: SyntaxLanguage,
        override_query: Option<&str>,
    ) -> Self {
        let mut parser = Parser::new();
        let ts_language = language.language();
        parser
            .set_language(&ts_language)
            .expect("Error loading grammar");

        let query = if let Some(oq) = override_query {
            Query::new(&ts_language, oq).unwrap_or_else(|_| {
                // Override failed to compile; fall back to built-in
                Query::new(&ts_language, language.query_source())
                    .expect("Error compiling built-in query")
            })
        } else {
            Query::new(&ts_language, language.query_source()).expect("Error compiling query")
        };

        Self {
            parser,
            query,
            language,
            last_tree: None,
        }
    }

    /// Create a new Syntax highlighter, detecting language from file path.
    /// Looks up override queries from the provided map keyed by language ID.
    pub fn new_from_path(path: Option<&str>) -> Option<Self> {
        Self::new_from_path_with_overrides(path, None)
    }

    /// Create a new Syntax highlighter with optional highlight query overrides.
    pub fn new_from_path_with_overrides(
        path: Option<&str>,
        overrides: Option<&std::collections::HashMap<String, String>>,
    ) -> Option<Self> {
        let lang = path.and_then(SyntaxLanguage::from_path)?;
        let override_query = overrides
            .and_then(|m| m.get(lang.language_id()))
            .map(|s| s.as_str());
        Some(Self::new_for_language_with_query(lang, override_query))
    }

    /// Create a Syntax from an LSP language identifier (e.g. "rust", "python").
    #[allow(dead_code)]
    pub fn new_from_language_id(id: &str) -> Option<Self> {
        Self::new_from_language_id_with_overrides(id, None)
    }

    /// Create a Syntax from an LSP language ID with optional highlight query overrides.
    pub fn new_from_language_id_with_overrides(
        id: &str,
        overrides: Option<&std::collections::HashMap<String, String>>,
    ) -> Option<Self> {
        let lang = SyntaxLanguage::from_language_id(id)?;
        let override_query = overrides
            .and_then(|m| m.get(lang.language_id()))
            .map(|s| s.as_str());
        Some(Self::new_for_language_with_query(lang, override_query))
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
        // Always do a full parse (no old_tree).  Passing the old tree for
        // incremental parsing requires calling tree.edit() with precise byte
        // offset deltas BEFORE reparsing.  Without tree.edit(), tree-sitter
        // assumes the text is unchanged and reuses stale nodes, producing
        // highlights with wrong byte offsets (garbled partial-word coloring).
        let tree = self
            .parser
            .parse(text, None)
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
                    let start = n.start_position();
                    result.push(BreadcrumbSymbol {
                        name,
                        kind: kind.to_string(),
                        line: start.row,
                        col: start.column,
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

#[allow(dead_code)]
impl Syntax {
    /// Find all direct scope-defining children of the scope node that starts
    /// at `parent_line`.  Returns sibling symbols at one level of nesting.
    /// Used as a tree-sitter fallback when LSP is unavailable.
    pub fn children_of_scope(&self, text: &str, parent_line: usize) -> Vec<BreadcrumbSymbol> {
        let tree = match self.last_tree.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        let scope_kinds = Self::scope_kinds_for(self.language);
        if scope_kinds.is_empty() {
            return vec![];
        }

        // Find the scope node at parent_line by walking the tree
        let point = tree_sitter::Point::new(parent_line, 0);
        let target = match tree.root_node().descendant_for_point_range(point, point) {
            Some(n) => n,
            None => return vec![],
        };

        // Walk up to find the actual scope node at this line
        let mut scope_node = None;
        let mut cur = Some(target);
        while let Some(n) = cur {
            if scope_kinds.contains(&n.kind()) && n.start_position().row == parent_line {
                scope_node = Some(n);
                break;
            }
            cur = n.parent();
        }

        let parent = match scope_node {
            Some(n) => n,
            None => return vec![],
        };

        // Walk all descendants looking for direct child scopes
        let mut result = Vec::new();
        Self::collect_child_scopes(&parent, text, scope_kinds, &mut result);
        result
    }

    /// Find all top-level scope-defining nodes in the file.
    pub fn top_level_scopes(&self, text: &str) -> Vec<BreadcrumbSymbol> {
        let tree = match self.last_tree.as_ref() {
            Some(t) => t,
            None => return vec![],
        };
        let scope_kinds = Self::scope_kinds_for(self.language);
        if scope_kinds.is_empty() {
            return vec![];
        }

        let root = tree.root_node();
        let mut result = Vec::new();
        for i in 0..root.child_count() {
            if let Some(child) = root.child(i as u32) {
                if scope_kinds.contains(&child.kind()) {
                    let name = Self::extract_node_name(&child, text);
                    if !name.is_empty() {
                        let start = child.start_position();
                        result.push(BreadcrumbSymbol {
                            name,
                            kind: child.kind().to_string(),
                            line: start.row,
                            col: start.column,
                        });
                    }
                }
            }
        }
        result
    }

    /// Collect direct child scope nodes (one level deep) of a parent node.
    fn collect_child_scopes(
        parent: &tree_sitter::Node,
        text: &str,
        scope_kinds: &[&str],
        out: &mut Vec<BreadcrumbSymbol>,
    ) {
        for i in 0..parent.child_count() {
            if let Some(child) = parent.child(i as u32) {
                if scope_kinds.contains(&child.kind()) {
                    let name = Self::extract_node_name(&child, text);
                    if !name.is_empty() {
                        let start = child.start_position();
                        out.push(BreadcrumbSymbol {
                            name,
                            kind: child.kind().to_string(),
                            line: start.row,
                            col: start.column,
                        });
                    }
                } else {
                    // Recurse into non-scope nodes (e.g. `impl_item` body block)
                    // to find nested scope children
                    Self::collect_child_scopes(&child, text, scope_kinds, out);
                }
            }
        }
    }
}

/// A symbol in the breadcrumb hierarchy (e.g. a function, struct, class).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbSymbol {
    pub name: String,
    pub kind: String,
    /// Start line (0-indexed) of the scope-defining node.
    pub line: usize,
    /// Start column (0-indexed) of the scope-defining node.
    pub col: usize,
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
        let code = "fn main() { let x = 42; }\nstruct Foo { bar: u32 }\nimpl Foo { fn baz(&self) -> bool { true } }";
        let highlights = syntax.parse(code);
        assert!(!highlights.is_empty());
        let kinds: std::collections::HashSet<&str> =
            highlights.iter().map(|(_, _, k)| k.as_str()).collect();
        assert!(kinds.contains("keyword"), "missing keyword");
        assert!(kinds.contains("function"), "missing function");
        assert!(kinds.contains("number"), "missing number");
        assert!(kinds.contains("type"), "missing type");
        assert!(
            kinds.contains("punctuation.bracket"),
            "missing punctuation.bracket"
        );
        assert!(kinds.contains("operator"), "missing operator");
        assert!(kinds.contains("boolean"), "missing boolean");
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
    fn test_reparse_preserves_captures() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
        let code1 = "fn main() { if true { let x = 42; } }";
        let h1 = syntax.parse(code1);
        let kinds1: std::collections::HashSet<&str> =
            h1.iter().map(|(_, _, k)| k.as_str()).collect();
        assert!(
            kinds1.contains("keyword.control"),
            "initial missing keyword.control"
        );
        assert!(kinds1.contains("keyword"), "initial missing keyword");
        assert!(kinds1.contains("boolean"), "initial missing boolean");

        // Simulate edit and re-parse
        let code2 = "fn main() { if true { let x = 43; } }";
        let h2 = syntax.parse(code2);
        let kinds2: std::collections::HashSet<&str> =
            h2.iter().map(|(_, _, k)| k.as_str()).collect();
        assert!(
            kinds2.contains("keyword.control"),
            "reparse missing keyword.control"
        );
        assert!(kinds2.contains("keyword"), "reparse missing keyword");
        assert!(kinds2.contains("boolean"), "reparse missing boolean");
    }

    #[test]
    fn test_highlights_sorted_by_start_byte() {
        let mut syntax = Syntax::new_for_language(SyntaxLanguage::Rust);
        let code = "fn main() { if true { let x = 42; } }\nstruct Foo { bar: u32 }";
        let highlights = syntax.parse(code);
        for pair in highlights.windows(2) {
            assert!(
                pair[0].0 <= pair[1].0,
                "highlights not sorted by start_byte: ({}, {}, {}) before ({}, {}, {})",
                pair[0].0,
                pair[0].1,
                pair[0].2,
                pair[1].0,
                pair[1].1,
                pair[1].2,
            );
        }
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
        assert!(
            kinds.contains(&"property"),
            "should highlight keys as property"
        );
        assert!(kinds.contains(&"number"), "should highlight numbers");
        assert!(kinds.contains(&"boolean"), "should highlight booleans");
        // Keys must not be overridden by string — check that key byte range has type, not string
        let key_highlights: Vec<_> = highlights
            .iter()
            .filter(|(s, e, _)| *s == 10 && *e == 18)
            .collect();
        assert!(
            key_highlights.iter().any(|(_, _, k)| k == "property"),
            "key should be property"
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

    #[test]
    fn test_language_id_round_trip() {
        // Every variant should round-trip through language_id → from_language_id
        let langs = [
            SyntaxLanguage::Rust,
            SyntaxLanguage::Python,
            SyntaxLanguage::JavaScript,
            SyntaxLanguage::TypeScript,
            SyntaxLanguage::Go,
            SyntaxLanguage::C,
            SyntaxLanguage::Cpp,
            SyntaxLanguage::CSharp,
            SyntaxLanguage::Java,
            SyntaxLanguage::Ruby,
            SyntaxLanguage::Lua,
            SyntaxLanguage::Bash,
            SyntaxLanguage::Json,
            SyntaxLanguage::Toml,
            SyntaxLanguage::Yaml,
            SyntaxLanguage::Html,
            SyntaxLanguage::Css,
            SyntaxLanguage::Markdown,
            SyntaxLanguage::Latex,
        ];
        for lang in &langs {
            let id = lang.language_id();
            let back = SyntaxLanguage::from_language_id(id);
            assert_eq!(
                back,
                Some(*lang),
                "round-trip failed for {:?} (id={id})",
                lang
            );
        }
    }

    #[test]
    fn test_override_query_used() {
        // A valid override query should be used instead of the built-in
        let override_q = "(line_comment) @comment";
        let mut syntax =
            Syntax::new_for_language_with_query(SyntaxLanguage::Rust, Some(override_q));
        let highlights = syntax.parse("// hello\nfn main() {}");
        let kinds: std::collections::HashSet<&str> =
            highlights.iter().map(|(_, _, k)| k.as_str()).collect();
        // Should have comment from override but NOT keyword (override doesn't capture keywords)
        assert!(
            kinds.contains("comment"),
            "override should capture comments"
        );
        assert!(
            !kinds.contains("keyword"),
            "override should NOT capture keywords"
        );
    }

    #[test]
    fn test_malformed_override_falls_back() {
        // A malformed override should fall back to the built-in query
        let bad_query = "THIS IS NOT A VALID QUERY !!!";
        let mut syntax = Syntax::new_for_language_with_query(SyntaxLanguage::Rust, Some(bad_query));
        let highlights = syntax.parse("fn main() { let x = 42; }");
        let kinds: std::collections::HashSet<&str> =
            highlights.iter().map(|(_, _, k)| k.as_str()).collect();
        // Should fall back to built-in and have keywords
        assert!(
            kinds.contains("keyword"),
            "fallback should capture keywords"
        );
    }

    #[test]
    fn test_override_map_lookup() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("rust".to_string(), "(line_comment) @comment".to_string());
        let mut syntax =
            Syntax::new_from_path_with_overrides(Some("test.rs"), Some(&overrides)).unwrap();
        let highlights = syntax.parse("// hello\nfn main() {}");
        let kinds: std::collections::HashSet<&str> =
            highlights.iter().map(|(_, _, k)| k.as_str()).collect();
        assert!(kinds.contains("comment"));
        assert!(!kinds.contains("keyword"));
    }
}
