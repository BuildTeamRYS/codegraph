//! TypeScript/JavaScript `LanguagePlugin` (first supported language family).
//!
//! Covers `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`.
//! TypeScript and JavaScript share the TypeScript grammar family:
//! the TSX grammar parses `.tsx`/`.jsx` (JSX dialect), the TypeScript
//! grammar handles the rest (JS is a syntactic subset of TS modulo edge
//! cases irrelevant to extraction).
//!
//! Extraction rules (all recorded in DECISIONS.md):
//! - Symbol kinds: function declarations (incl. generators), methods,
//!   classes, interfaces, type aliases, enums, and `const x = () => ...`
//!   / `const x = function ...` bindings, exported or not. Nested named
//!   symbols are extracted too.
//! - Docstrings: the `/** ... */` block immediately preceding the statement
//!   (or its `export` wrapper) is attached, stripped of comment punctuation.
//! - Symbol content_hash covers the doc comment AND the declaration text, so
//!   editing only a docstring still invalidates derived facts.
//! - Imports: one record per binding (named/default/namespace/side-effect)
//!   plus re-exports. Used by the resolve pass, not stored as edges.
//! - Call sites (`x()`, `obj.x()`, `new X()`) are recorded raw; the resolve
//!   pass at index time turns them into `edges(kind='calls')`.
//! - `.d.ts` and minified/generated files are flagged `is_generated`.
//! - Files whose tree contains ERROR nodes are still indexed best-effort but
//!   flagged via `had_errors`; the caller records an indexing gap.

use std::path::Path;

use codegraph_core::{hash_bytes, CallSite, ImportRecord, SymbolKind, SymbolRecord};
use codegraph_parse::{is_vendor_path, LanguagePlugin, ParseError, ParsedFile};
use tree_sitter::{Node, Parser};

/// TypeScript/JavaScript dialect plugin. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct TsPlugin;

impl TsPlugin {
    pub fn new() -> Self {
        Self
    }

    fn is_jsx(rel_path: &str) -> bool {
        matches!(
            Path::new(rel_path).extension().and_then(|e| e.to_str()),
            Some(ext) if ext.eq_ignore_ascii_case("tsx") || ext.eq_ignore_ascii_case("jsx")
        )
    }
}

impl LanguagePlugin for TsPlugin {
    fn language_for(&self, rel_path: &str) -> Option<&'static str> {
        let ext = Path::new(rel_path)
            .extension()
            .and_then(|e| e.to_str())?
            .to_ascii_lowercase();
        match ext.as_str() {
            "ts" | "mts" | "cts" => Some("typescript"),
            "tsx" => Some("tsx"),
            "js" | "mjs" | "cjs" => Some("javascript"),
            "jsx" => Some("jsx"),
            _ => None,
        }
    }

    fn classify(&self, rel_path: &str) -> (bool, bool) {
        let is_vendor = is_vendor_path(rel_path);
        let name = Path::new(rel_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_generated = name.ends_with(".d.ts")
            || name.contains(".generated.")
            || name.contains(".g.ts")
            || name.contains(".min.");
        (is_generated, is_vendor)
    }

    fn extract(&self, rel_path: &str, bytes: &[u8]) -> Result<ParsedFile, ParseError> {
        let lang = self
            .language_for(rel_path)
            .ok_or_else(|| ParseError::Parser(format!("unsupported file {rel_path}")))?;
        let is_jsx = Self::is_jsx(rel_path);
        let mut parser = Parser::new();
        let language = if is_jsx {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };
        parser
            .set_language(&language.into())
            .map_err(|e| ParseError::Parser(e.to_string()))?;
        let tree = parser
            .parse(bytes, None)
            .ok_or_else(|| ParseError::Parser("parser returned no tree".into()))?;
        let had_errors = tree.root_node().has_error();
        let src = std::str::from_utf8(bytes)
            .map_err(|e| ParseError::Parser(format!("not utf-8: {e}")))?;

        let (is_generated, is_vendor) = self.classify(rel_path);
        let mut ex = Extractor {
            path: rel_path,
            src: src.as_bytes(),
            symbols: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
        };
        ex.visit(tree.root_node(), &mut Vec::new(), None);

        Ok(ParsedFile {
            path: rel_path.to_string(),
            lang: lang.to_string(),
            content_hash: hash_bytes(bytes),
            size: bytes.len() as i64,
            is_generated,
            is_vendor,
            symbols: ex.symbols,
            imports: ex.imports,
            calls: ex.calls,
            had_errors,
        })
    }
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
    calls: Vec<CallSite>,
}

impl<'s> Extractor<'s> {
    /// `doc` is Some when the parent (e.g. `export_statement`) already found
    /// the doc comment; otherwise we look before `stmt`.
    fn visit(&mut self, node: Node, scope: &mut Vec<String>, doc: Option<String>) {
        let src = self.src;
        let kind = node.kind();

        match kind {
            "import_statement" => {
                self.collect_import_bindings(&node);
                return; // no symbols inside an import
            }
            "export_statement" => {
                if let Some(src_node) = node.child_by_field_name("source") {
                    // `export ... from 'x'`: re-export, no local binding created.
                    self.imports.push(ImportRecord {
                        path: self.path.to_string(),
                        source: unquote(node_text(&src_node, src)),
                        kind: "reexport".to_string(),
                        local_name: None,
                        imported_name: None,
                        line: node.start_position().row as u32 + 1,
                    });
                    return;
                }
                let doc = docstring_for(&node, src);
                self.walk_children_skipping_decorators(&node, scope, doc);
                return;
            }
            "call_expression" => {
                if let Some(f) = node.child_by_field_name("function") {
                    self.record_callee(&f, false, node.start_position().row as u32 + 1);
                }
            }
            "new_expression" => {
                if let Some(f) = node.child_by_field_name("constructor") {
                    self.record_callee(&f, true, node.start_position().row as u32 + 1);
                }
            }
            "function_declaration" | "generator_function_declaration" => {
                self.emit_function(&node, scope, doc);
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
                // Visit members to pick up methods (scoped under this class).
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "class_body"
                        || child.kind() == "interface_body"
                        || child.kind() == "enum_body"
                    {
                        let child_cursor_target = child;
                        for member in
                            child_cursor_target.named_children(&mut child_cursor_target.walk())
                        {
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
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, scope, None);
        }
    }

    fn walk_children_skipping_decorators(
        &mut self,
        node: &Node,
        scope: &mut Vec<String>,
        doc: Option<String>,
    ) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "decorator" {
                continue;
            }
            self.visit(child, scope, doc.clone());
        }
    }

    /// Record the callee of a call/construct expression. Only a plain
    /// identifier or a member expression yields a name; anything else
    /// (calls of expressions, etc.) is skipped — recorded as nothing,
    /// which is the honest answer.
    fn record_callee(&mut self, f: &Node, is_construct: bool, line: u32) {
        match f.kind() {
            "identifier" => self.calls.push(CallSite {
                path: self.path.to_string(),
                callee_name: node_text(f, self.src).to_string(),
                qualifier: None,
                is_construct,
                line,
            }),
            "member_expression" => {
                let object = f.child_by_field_name("object");
                let property = f.child_by_field_name("property");
                if let Some(property) = property {
                    self.calls.push(CallSite {
                        path: self.path.to_string(),
                        callee_name: node_text(&property, self.src).to_string(),
                        qualifier: object.map(|o| node_text(&o, self.src).to_string()),
                        is_construct,
                        line,
                    });
                }
            }
            _ => {}
        }
    }

    /// One binding record per imported name.
    fn collect_import_bindings(&mut self, node: &Node) {
        let src = self.src;
        let path = self.path.to_string();
        let line = node.start_position().row as u32 + 1;
        let Some(src_node) = node.child_by_field_name("source") else {
            return;
        };
        let source = unquote(node_text(&src_node, src));
        let mut cursor = node.walk();
        let mut saw_binding = false;
        for child in node.named_children(&mut cursor) {
            if child.kind() != "import_clause" {
                continue;
            }
            let mut clause_cursor = child.walk();
            for clause in child.named_children(&mut clause_cursor) {
                match clause.kind() {
                    "identifier" => {
                        // `import def from 'x'` — default import.
                        saw_binding = true;
                        self.imports.push(ImportRecord {
                            path: path.clone(),
                            source: source.clone(),
                            kind: "default".to_string(),
                            local_name: Some(node_text(&clause, src).to_string()),
                            imported_name: None,
                            line,
                        });
                    }
                    "namespace_import" => {
                        saw_binding = true;
                        // `import * as ns from 'x'` — first identifier child.
                        let mut ns_cursor = clause.walk();
                        let local = clause
                            .named_children(&mut ns_cursor)
                            .find(|c| c.kind() == "identifier")
                            .map(|n| node_text(&n, src).to_string());
                        self.imports.push(ImportRecord {
                            path: path.clone(),
                            source: source.clone(),
                            kind: "namespace".to_string(),
                            local_name: local,
                            imported_name: None,
                            line,
                        });
                    }
                    "named_imports" => {
                        let mut named_cursor = clause.walk();
                        for spec in clause.named_children(&mut named_cursor) {
                            if spec.kind() != "import_specifier" {
                                continue;
                            }
                            let imported = spec
                                .child_by_field_name("name")
                                .map(|n| node_text(&n, src).to_string());
                            let alias = spec
                                .child_by_field_name("alias")
                                .map(|n| node_text(&n, src).to_string());
                            saw_binding = true;
                            self.imports.push(ImportRecord {
                                path: path.clone(),
                                source: source.clone(),
                                kind: "named".to_string(),
                                local_name: alias.clone().or_else(|| imported.clone()),
                                imported_name: imported,
                                line,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        if !saw_binding {
            // `import 'side-effect'` — no bindings, but the specifier matters.
            self.imports.push(ImportRecord {
                path,
                source,
                kind: "side_effect".to_string(),
                local_name: None,
                imported_name: None,
                line,
            });
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

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '\'' || c == '"' || c == '`')
        .to_string()
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
