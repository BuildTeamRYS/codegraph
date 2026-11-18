//! Core types shared across codegraph crates.
//!
//! The data model itself lives in PLAN.md section 3; this crate only holds
//! the small value types so every other crate agrees on them.

use serde::{Deserialize, Serialize};

/// Schema version written into `meta.schema_version`. Bump on any migration.
pub const SCHEMA_VERSION: u32 = 2;

/// Content hash used everywhere in the index (files, symbols, valid_hash).
/// blake3 hex, full 64 chars.
pub type ContentHash = String;

pub fn hash_bytes(bytes: &[u8]) -> ContentHash {
    blake3::hash(bytes).to_hex().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Certain,
    Inferred,
    Unknown,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::Certain => "certain",
            Confidence::Inferred => "inferred",
            Confidence::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Class,
    Method,
    Interface,
    TypeAlias,
    Enum,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Class => "class",
            SymbolKind::Method => "method",
            SymbolKind::Interface => "interface",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Enum => "enum",
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "function" => Some(SymbolKind::Function),
            "class" => Some(SymbolKind::Class),
            "method" => Some(SymbolKind::Method),
            "interface" => Some(SymbolKind::Interface),
            "type_alias" => Some(SymbolKind::TypeAlias),
            "enum" => Some(SymbolKind::Enum),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRecord {
    /// Repo-relative path, forward slashes.
    pub path: String,
    pub kind: SymbolKind,
    pub name: String,
    pub qualified_name: String,
    /// Terse signature, e.g. `(user, pw) -> Session`.
    pub signature: String,
    /// 1-based, inclusive.
    pub start_line: u32,
    /// 1-based, inclusive.
    pub end_line: u32,
    pub docstring: Option<String>,
    pub content_hash: ContentHash,
}

/// A single import binding. One row per binding, not per statement:
/// `import def, { a, b as c } from 'm'` produces three records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRecord {
    /// Repo-relative path of the importing file.
    pub path: String,
    /// Module specifier as written, e.g. `./crypto`.
    pub source: String,
    /// `named | default | namespace | side_effect | reexport`
    pub kind: String,
    /// Local binding name; None for side-effect imports and re-exports.
    pub local_name: Option<String>,
    /// Imported (exported-from-source) name for named bindings; None otherwise.
    pub imported_name: Option<String>,
    pub line: u32,
}

/// A raw call/construct site, before cross-file resolution. Resolved into
/// `edges` by the resolve pass at index time (phase 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSite {
    pub path: String,
    /// Callee name as written (identifier text).
    pub callee_name: String,
    /// Object text for member calls (`crypto`, `this`, `this.items`); None for plain calls.
    pub qualifier: Option<String>,
    /// True for `new X(...)`, false for `x(...)`.
    pub is_construct: bool,
    pub line: u32,
}

/// Edge kinds, from PLAN.md section 3.
pub const EDGE_KINDS: &[&str] = &[
    "calls",
    "imports",
    "defines",
    "inherits",
    "reads",
    "writes",
    "tested_by",
    "handles_route",
    "depends_on",
];

/// HEAD commit of the git repo at `repo`, or None if unavailable/not a repo.
pub fn git_head_commit(repo: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
