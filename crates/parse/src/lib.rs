//! Tree-sitter based extraction. One pass over the tree per file.
//!
//! Extraction rules (all recorded in DECISIONS.md):
//! - Only `.ts`, `.tsx`, `.mts`, `.cts` files are parsed for now.
//! - Symbol kinds: function declarations (incl. generators), methods,
//!   classes, interfaces, type aliases, enums, and `const x = () => ...`
//!   / `const x = function ...` bindings exported or not.
//! - Docstrings: the `/** ... */` block immediately preceding the statement
//!   (or its `export` wrapper) is attached, stripped of comment punctuation.
//! - Symbol content_hash covers the doc comment AND the declaration text, so
//!   editing only a docstring still invalidates derived facts.
//! - Imports: static `import ... from 'x'` and `export ... from 'x'` sources.
//! - Files whose tree contains ERROR nodes are still indexed best-effort but
//!   recorded as indexing gaps by the caller (never silently complete).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use codegraph_core::{hash_bytes, ImportRecord, SymbolKind, SymbolRecord};
use ignore::WalkBuilder;
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unreadable file {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{0}")]
    TreeSitter(String),
}

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub path: String,
    pub lang: String,
    pub content_hash: String,
    pub size: i64,
    pub is_generated: bool,
    pub is_vendor: bool,
    pub symbols: Vec<SymbolRecord>,
    pub imports: Vec<ImportRecord>,
    /// True if the tree-sitter tree contained ERROR nodes.
    pub had_errors: bool,
}

#[derive(Debug)]
pub enum FileOutcome {
    /// Content hash matched the index; no parsing was done.
    Unchanged {
        path: String,
    },
    Parsed(ParsedFile),
    /// File could not be read at all; caller records a gap.
    Failed {
        path: String,
        error: String,
    },
}

#[derive(Debug, Default)]
pub struct ScanOutput {
    pub outcomes: Vec<FileOutcome>,
    pub seen_paths: HashSet<String>,
    pub scanned: u64,
}

const TS_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts"];
const VENDOR_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    "third_party",
    "dist",
    "build",
    "out",
];

pub fn is_typescript(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TS_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn classify(path: &Path) -> (bool, bool) {
    let s = path.to_string_lossy();
    let is_vendor = VENDOR_DIRS
        .iter()
        .any(|d| s.split(['/', '\\']).any(|c| c == *d));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_generated = name.ends_with(".d.ts")
        || name.contains(".generated.")
        || name.contains(".g.ts")
        || name.contains(".min.");
    (is_generated, is_vendor)
}

/// Walk `root`, skipping unchanged files (same content hash in `existing`),
/// parsing everything else. Caller decides what to do with outcomes.
pub fn scan(
    root: &Path,
    existing: &HashMap<String, (i64, String)>,
) -> Result<ScanOutput, ParseError> {
    let root = root.canonicalize().map_err(|e| ParseError::Io {
        path: root.display().to_string(),
        source: e,
    })?;
    let mut output = ScanOutput::default();

    let mut builder = WalkBuilder::new(&root);
    builder
        .standard_filters(true) // respects .gitignore and global ignores
        .hidden(false) // hidden config files are still source; .git is always skipped
        .follow_links(false)
        .sort_by_file_path(|a, b| a.cmp(b));

    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                output.outcomes.push(FileOutcome::Failed {
                    path: root.display().to_string(),
                    error: format!("walk error: {e}"),
                });
                continue;
            }
        };
        let path = entry.path();
        if !path.is_file() || !is_typescript(path) {
            continue;
        }
        output.scanned += 1;
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        output.seen_paths.insert(rel.clone());

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(source) => {
                output.outcomes.push(FileOutcome::Failed {
                    path: rel,
                    error: source.to_string(),
                });
                continue;
            }
        };
        let content_hash = hash_bytes(&bytes);
        if let Some((_id, existing_hash)) = existing.get(&rel) {
            if *existing_hash == content_hash {
                output.outcomes.push(FileOutcome::Unchanged { path: rel });
                continue;
            }
        }
        match parse_bytes(&rel, &bytes) {
            Ok(parsed) => output.outcomes.push(FileOutcome::Parsed(parsed)),
            Err(e) => output.outcomes.push(FileOutcome::Failed {
                path: rel,
                error: e.to_string(),
            }),
        }
    }
    Ok(output)
}

/// Parse a single in-memory TypeScript file. Useful for tests and fixtures.
pub fn parse_bytes(rel_path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
    let is_tsx = rel_path.ends_with(".tsx");
    let mut parser = Parser::new();
    let language = if is_tsx {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };
    parser
        .set_language(&language.into())
        .map_err(|e| ParseError::TreeSitter(e.to_string()))?;
    let tree = parser
        .parse(bytes, None)
        .ok_or_else(|| ParseError::TreeSitter("parser returned no tree".into()))?;
    let had_errors = tree.root_node().has_error();
    let src = std::str::from_utf8(bytes)
        .map_err(|e| ParseError::TreeSitter(format!("not utf-8: {e}")))?;

    let (is_generated, is_vendor) = classify(Path::new(rel_path));
    let mut ex = Extractor {
        path: rel_path,
        src: src.as_bytes(),
        symbols: Vec::new(),
        imports: Vec::new(),
    };
    ex.visit(tree.root_node(), &mut Vec::new(), None);

    Ok(ParsedFile {
        path: rel_path.to_string(),
        lang: if is_tsx { "tsx" } else { "typescript" }.to_string(),
        content_hash: hash_bytes(bytes),
        size: bytes.len() as i64,
        is_generated,
        is_vendor,
        symbols: ex.symbols,
        imports: ex.imports,
        had_errors,
    })
}

fn node_text<'s>(node: &Node, src: &'s [u8]) -> &'s str {
    node.utf8_text(src).unwrap_or("")
}

fn clean_docstring(comment: &str) -> Option<String> {
    let body = comment.trim();
    let body = body.strip_prefix("/**").unwrap_or(body);
    let body = body.strip_suffix("*/").unwrap_or(body);
    let lines: Vec<&str> = body
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .collect();
    let joined = lines
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Find the `/** ... */` comment immediately preceding `stmt`.
fn docstring_for(stmt: &Node, src: &[u8]) -> Option<String> {
    let prev = stmt.prev_named_sibling()?;
    if prev.kind() == "comment" {
        let text = node_text(&prev, src);
        if text.trim_start().starts_with("/**") {
            return clean_docstring(text);
        }
    }
    None
}

fn params_signature(params_node: &Node, src: &[u8]) -> String {
    let mut names = Vec::new();
    let mut cursor = params_node.walk();
    for child in params_node.named_children(&mut cursor) {
        let name = match child.kind() {
            "required_parameter" | "optional_parameter" => child
                .child_by_field_name("pattern")
                .or_else(|| child.named_child(0))
                .map(|p| param_pattern_name(&p, src)),
            "rest_parameter" => child
                .named_child(0)
                .map(|p| format!("...{}", param_pattern_name(&p, src))),
            "identifier" => Some(node_text(&child, src).to_string()),
            _ => None,
        };
        if let Some(n) = name {
            names.push(n);
        }
    }
    format!("({})", names.join(", "))
}

/// `({a, b})` destructuring stays as-is; plain identifiers as-is.
fn param_pattern_name(pattern: &Node, src: &[u8]) -> String {
    match pattern.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            node_text(pattern, src).to_string()
        }
        _ => node_text(pattern, src)
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
    }
}

fn return_type_signature(decl: &Node, src: &[u8]) -> Option<String> {
    let rt = decl.child_by_field_name("return_type")?;
    let text = node_text(&rt, src).trim();
    let text = text.strip_prefix(':').unwrap_or(text).trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn function_signature(decl: &Node, src: &[u8]) -> String {
    let params = decl
        .child_by_field_name("parameters")
        .map(|p| params_signature(&p, src))
        .unwrap_or_else(|| "()".to_string());
    match return_type_signature(decl, src) {
        Some(rt) => format!("{params} -> {rt}"),
        None => params,
    }
}

struct Extractor<'s> {
    path: &'s str,
    src: &'s [u8],
    symbols: Vec<SymbolRecord>,
    imports: Vec<ImportRecord>,
}

impl<'s> Extractor<'s> {
    /// `doc` is Some when the parent (e.g. `export_statement`) already found
    /// the doc comment; otherwise we look before `stmt`.
    fn visit(&mut self, node: Node, scope: &mut Vec<String>, doc: Option<String>) {
        let src = self.src;
        let kind = node.kind();

        match kind {
            "import_statement" | "export_statement" => {
                if let Some(src_node) = node.child_by_field_name("source") {
                    // `import ... from 'x'` and `export ... from 'x'` both carry a source.
                    self.imports.push(ImportRecord {
                        path: self.path.to_string(),
                        source: node_text(&src_node, src)
                            .trim_matches(|c| c == '\'' || c == '"')
                            .to_string(),
                        line: node.start_position().row as u32 + 1,
                    });
                }
                let doc = docstring_for(&node, src);
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "decorator" {
                        continue;
                    }
                    self.visit(child, scope, doc.clone());
                }
                return;
            }
            "function_declaration" | "generator_function_declaration" => {
                self.emit_function(&node, scope, doc);
                return; // bodies contain no named symbols in TS
            }
            "method_definition" => {
                let name_node = node.child_by_field_name("name");
                if let Some(name_node) = name_node {
                    let name = node_text(&name_node, src).to_string();
                    let signature = function_signature(&node, src);
                    let doc = self.take_doc(&node, doc);
                    self.emit(
                        &node,
                        &node,
                        SymbolKind::Method,
                        &name,
                        scope,
                        signature,
                        doc,
                    );
                }
                return;
            }
            "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration" => {
                let kind = match kind {
                    "class_declaration" | "abstract_class_declaration" => SymbolKind::Class,
                    "interface_declaration" => SymbolKind::Interface,
                    "type_alias_declaration" => SymbolKind::TypeAlias,
                    _ => SymbolKind::Enum,
                };
                let Some(name_node) = node.child_by_field_name("name") else {
                    return;
                };
                let name = node_text(&name_node, src).to_string();
                let keyword = match kind {
                    SymbolKind::Class => "class",
                    SymbolKind::Interface => "interface",
                    SymbolKind::TypeAlias => "type",
                    _ => "enum",
                };
                let signature = format!("{keyword} {name}");
                let doc = self.take_doc(&node, doc);
                self.emit(&node, &node, kind, &name, scope, signature, doc);
                scope.push(name);
                // Visit members to pick up methods (pushed into the class scope).
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "class_body"
                        || child.kind() == "interface_body"
                        || child.kind() == "enum_body"
                    {
                        for member in child.named_children(&mut child.walk()) {
                            self.visit(member, scope, None);
                        }
                    }
                }
                scope.pop();
                return;
            }
            "lexical_declaration" | "variable_declaration" => {
                if let Some((name, value)) = declaration_binding(&node, src) {
                    let signature = function_signature(&value, src);
                    let doc = self.take_doc(&node, doc);
                    self.emit(
                        &value,
                        &node,
                        SymbolKind::Function,
                        &name,
                        scope,
                        signature,
                        doc,
                    );
                }
                return;
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, scope, None);
        }
    }

    fn take_doc(&self, stmt: &Node, doc: Option<String>) -> Option<String> {
        doc.or_else(|| docstring_for(stmt, self.src))
    }

    fn emit_function(&mut self, node: &Node, scope: &[String], doc: Option<String>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = node_text(&name_node, self.src).to_string();
        let signature = function_signature(node, self.src);
        let doc = self.take_doc(node, doc);
        self.emit(
            node,
            node,
            SymbolKind::Function,
            &name,
            scope,
            signature,
            doc,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        hash_node: &Node,
        range_node: &Node,
        kind: SymbolKind,
        name: &str,
        scope: &[String],
        signature: String,
        doc: Option<String>,
    ) {
        let qualified = if scope.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", scope.join("."), name)
        };
        // Hash doc comment + declaration text so doc edits invalidate derived facts.
        let mut hasher_input = Vec::new();
        if let Some(d) = &doc {
            hasher_input.extend_from_slice(d.as_bytes());
            hasher_input.push(0);
        }
        hasher_input.extend_from_slice(node_text(hash_node, self.src).as_bytes());
        self.symbols.push(SymbolRecord {
            path: self.path.to_string(),
            kind,
            name: name.to_string(),
            qualified_name: qualified,
            signature,
            start_line: range_node.start_position().row as u32 + 1,
            end_line: range_node.end_position().row as u32 + 1,
            docstring: doc,
            content_hash: hash_bytes(&hasher_input),
        });
    }
}

/// Extract the `x` from `const x = () => ...` / `const x = function () {...}`.
fn declaration_binding<'tree>(
    declaration: &Node<'tree>,
    src: &[u8],
) -> Option<(String, Node<'tree>)> {
    let mut cursor = declaration.walk();
    for declarator in declaration
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "variable_declarator")
    {
        let value = match declarator.child_by_field_name("value") {
            Some(v) => v,
            None => continue,
        };
        if matches!(value.kind(), "arrow_function" | "function_expression") {
            if let Some(name_node) = declarator.child_by_field_name("name") {
                if name_node.kind() == "identifier" {
                    return Some((node_text(&name_node, src).to_string(), value));
                }
            }
        }
    }
    None
}
