use tree_sitter::{Node, Parser};

fn find_error<'t>(node: Node<'t>) {
    if node.kind() == "ERROR" || node.is_error() {
        let s = node.start_position();
        let e = node.end_position();
        println!(
            "ERROR node lines {}-{} (range {:?}..{:?})",
            s.row + 1,
            e.row + 1,
            s,
            e
        );
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        find_error(child);
    }
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: debug_file <file.tsx>");
    let bytes = std::fs::read(&path).unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())
        .unwrap();
    let tree = parser.parse(&bytes, None).unwrap();
    let root = tree.root_node();
    find_error(root);
}
