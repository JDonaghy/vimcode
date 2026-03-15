fn main() {
    // Compile vendored tree-sitter-latex grammar (v0.3.0, language version 14)
    cc::Build::new()
        .include("vendor/tree-sitter-latex/src")
        .file("vendor/tree-sitter-latex/src/parser.c")
        .file("vendor/tree-sitter-latex/src/scanner.c")
        .warnings(false)
        .compile("tree_sitter_latex");
}
