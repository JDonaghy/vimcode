use tree_sitter::{Parser, Query, QueryCursor};

pub struct Syntax {
    parser: Parser,
    query: Query,
}

impl Syntax {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        let language = tree_sitter_rust::language();
        parser
            .set_language(language)
            .expect("Error loading Rust grammar");

        let query_source = "
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
        ";
        let query = Query::new(language, query_source).expect("Error compiling query");

        Self { parser, query }
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
