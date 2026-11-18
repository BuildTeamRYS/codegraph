use codegraph_core::{hash_bytes, Confidence, SymbolKind};

#[test]
fn hash_is_deterministic_and_distinct() {
    assert_eq!(hash_bytes(b"hello"), hash_bytes(b"hello"));
    assert_ne!(hash_bytes(b"hello"), hash_bytes(b"world"));
    assert_eq!(hash_bytes(b"x").len(), 64);
}

#[test]
fn symbol_kind_roundtrip() {
    for kind in [
        "function",
        "class",
        "method",
        "interface",
        "type_alias",
        "enum",
    ] {
        assert_eq!(SymbolKind::from_str(kind).unwrap().as_str(), kind);
    }
    assert!(SymbolKind::from_str("macro").is_none());
}

#[test]
fn confidence_strings() {
    assert_eq!(Confidence::Certain.as_str(), "certain");
    assert_eq!(Confidence::Inferred.as_str(), "inferred");
    assert_eq!(Confidence::Unknown.as_str(), "unknown");
}
