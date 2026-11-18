use tree_sitter::Parser;
fn main() {
    let code = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let tree = parser.parse(&code, None).unwrap();
    println!("{}", tree.root_node().to_sexp());
}
