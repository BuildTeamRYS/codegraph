//! Surgical, format-aware config editing.
//!
//! Guarantees (tested in `tests/config_merge.rs`):
//! - other servers, comments, formatting and key order survive byte-identically;
//! - a file we cannot parse is never modified ([`EditError::Unparseable`]);
//! - a `"key": <non-object>` collision is never overwritten
//!   ([`EditError::Invalid`]) — the file is left untouched;
//! - empty parents left behind by a removal are pruned.
//!
//! JSON is edited with a lossless CST (`jsonc-parser`), TOML with
//! `toml_edit` (comment-preserving).

use serde_json::{json, Value};

use crate::spec::{AgentSpec, ConfigFormat};

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("unparseable config ({format}): {reason}")]
    Unparseable {
        format: &'static str,
        reason: String,
    },
    #[error("invalid config shape: {0}")]
    Invalid(String),
    #[error("entry not present")]
    NotPresent,
}

fn format_name(spec: &AgentSpec) -> &'static str {
    spec.format.as_str()
}

/// The JSON entry for codegraph in this agent's config, as `CstInputValue`.
fn entry_cst(spec: &AgentSpec, command: &str, args: &[String]) -> jsonc_parser::cst::CstInputValue {
    use jsonc_parser::cst::CstInputValue as V;
    let cmd_array: Vec<V> = std::iter::once(command.to_string())
        .chain(args.iter().cloned())
        .map(V::String)
        .collect();
    if spec.entry.command_array {
        // OpenCode: { "type": "local", "command": [bin, ...args] }
        return V::Object(vec![
            ("type".into(), V::String("local".into())),
            ("command".into(), V::Array(cmd_array)),
        ]);
    }
    let mut obj: Vec<(String, V)> = Vec::with_capacity(3);
    if spec.entry.include_type {
        obj.push(("type".into(), V::String("stdio".into())));
    }
    obj.push(("command".into(), V::String(command.to_string())));
    obj.push((
        "args".into(),
        V::Array(args.iter().cloned().map(V::String).collect()),
    ));
    let _ = cmd_array;
    V::Object(obj)
}

/// Read the current codegraph entry (if any), as JSON, for status display.
pub fn read_entry(text: &str, spec: &AgentSpec) -> Result<Option<Value>, EditError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    match spec.format {
        ConfigFormat::Json | ConfigFormat::Jsonc => read_entry_jsonc(text, spec),
        ConfigFormat::Toml => read_entry_toml(text, spec),
    }
}

fn read_entry_jsonc(text: &str, spec: &AgentSpec) -> Result<Option<Value>, EditError> {
    let root = jsonc_parser::cst::CstRootNode::parse(text, &Default::default()).map_err(|e| {
        EditError::Unparseable {
            format: format_name(spec),
            reason: e.to_string(),
        }
    })?;
    let Some(mut obj) = root.object_value() else {
        return Err(EditError::Invalid("top level is not an object".into()));
    };
    for key in spec.entry.servers_key {
        match obj.get(key) {
            Some(prop) => match prop.object_value() {
                Some(child) => obj = child,
                None => return Err(EditError::Invalid(format!("`{key}` is not an object"))),
            },
            None => return Ok(None),
        }
    }
    Ok(obj
        .get("codegraph")
        .and_then(|p| p.value()?.to_serde_value()))
}

fn read_entry_toml(text: &str, spec: &AgentSpec) -> Result<Option<Value>, EditError> {
    let doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|e: toml_edit::TomlError| EditError::Unparseable {
                format: format_name(spec),
                reason: e.to_string(),
            })?;
    // TOML configs nest one level only (`[mcp_servers.<name>]`).
    let Some(key) = spec.entry.servers_key.first() else {
        return Ok(None);
    };
    if spec.entry.servers_key.len() != 1 {
        return Err(EditError::Invalid(
            "multi-level TOML nesting is not supported".into(),
        ));
    }
    let Some(servers) = doc.as_table().get(key).and_then(|i| i.as_table_like()) else {
        return Ok(None);
    };
    let Some(entry) = servers.get("codegraph").and_then(|i| i.as_table_like()) else {
        return Ok(None);
    };
    let command = entry
        .get("command")
        .and_then(|i| i.as_str())
        .unwrap_or_default();
    let args: Vec<Value> = entry
        .get("args")
        .and_then(|i| i.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(Value::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(json!({ "command": command, "args": args })))
}

/// Insert or replace the codegraph entry. Returns the new file text.
pub fn upsert_entry(
    text: &str,
    spec: &AgentSpec,
    command: &str,
    args: &[String],
) -> Result<String, EditError> {
    if text.trim().is_empty() {
        // Empty file: start from an empty object / empty document.
        return match spec.format {
            ConfigFormat::Json | ConfigFormat::Jsonc => upsert_entry("{}\n", spec, command, args),
            ConfigFormat::Toml => {
                // An empty TOML document is valid and parses fine; fall
                // through to the normal path instead of recursing.
                return upsert_entry_toml("", spec, command, args);
            }
        };
    }
    match spec.format {
        ConfigFormat::Json | ConfigFormat::Jsonc => upsert_entry_jsonc(text, spec, command, args),
        ConfigFormat::Toml => upsert_entry_toml(text, spec, command, args),
    }
}

fn upsert_entry_jsonc(
    text: &str,
    spec: &AgentSpec,
    command: &str,
    args: &[String],
) -> Result<String, EditError> {
    let root = jsonc_parser::cst::CstRootNode::parse(text, &Default::default()).map_err(|e| {
        EditError::Unparseable {
            format: format_name(spec),
            reason: e.to_string(),
        }
    })?;
    let Some(mut obj) = root.object_value() else {
        return Err(EditError::Invalid("top level is not an object".into()));
    };
    for key in spec.entry.servers_key {
        obj = obj.object_value_or_create(key).ok_or_else(|| {
            EditError::Invalid(format!(
                "`{key}` exists but is not an object — left untouched"
            ))
        })?;
    }
    let entry = entry_cst(spec, command, args);
    match obj.get("codegraph") {
        Some(prop) => prop.set_value(entry),
        None => {
            obj.append("codegraph", entry);
        }
    }
    Ok(root.to_string())
}

fn upsert_entry_toml(
    text: &str,
    spec: &AgentSpec,
    command: &str,
    args: &[String],
) -> Result<String, EditError> {
    let mut doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|e: toml_edit::TomlError| EditError::Unparseable {
                format: format_name(spec),
                reason: e.to_string(),
            })?;
    let key = spec.entry.servers_key[0];
    let mut table = toml_edit::Table::new();
    if spec.entry.include_type {
        table["type"] = toml_edit::value("stdio");
    }
    table["command"] = toml_edit::value(command);
    let mut arr = toml_edit::Array::new();
    for a in args {
        arr.push(a.as_str());
    }
    table["args"] = toml_edit::value(arr);
    let servers = doc
        .as_table_mut()
        .entry(key)
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(servers) = servers.as_table_like_mut() else {
        return Err(EditError::Invalid(format!(
            "`{key}` exists but is not a table — left untouched"
        )));
    };
    servers.insert("codegraph", toml_edit::Item::Table(table));
    Ok(doc.to_string())
}

/// Remove the codegraph entry (and empty parents). Returns the new text.
pub fn remove_entry(text: &str, spec: &AgentSpec) -> Result<String, EditError> {
    if text.trim().is_empty() {
        return Err(EditError::NotPresent);
    }
    match spec.format {
        ConfigFormat::Json | ConfigFormat::Jsonc => remove_entry_jsonc(text, spec),
        ConfigFormat::Toml => remove_entry_toml(text, spec),
    }
}

fn remove_entry_jsonc(text: &str, spec: &AgentSpec) -> Result<String, EditError> {
    let root = jsonc_parser::cst::CstRootNode::parse(text, &Default::default()).map_err(|e| {
        EditError::Unparseable {
            format: format_name(spec),
            reason: e.to_string(),
        }
    })?;
    let Some(obj) = root.object_value() else {
        return Err(EditError::Invalid("top level is not an object".into()));
    };
    // Descend through every level, recording the chain so empty parents
    // can be pruned bottom-up.
    let mut chain: Vec<jsonc_parser::cst::CstObjectProp> = Vec::new();
    let mut obj = obj;
    for key in spec.entry.servers_key {
        match obj.get(key) {
            Some(prop) => match prop.object_value() {
                Some(child) => {
                    chain.push(prop);
                    obj = child;
                }
                None => {
                    return Err(EditError::Invalid(format!(
                        "`{key}` is not an object — left untouched"
                    )))
                }
            },
            None => return Err(EditError::NotPresent),
        }
    }
    match obj.get("codegraph") {
        Some(prop) => prop.remove(),
        None => return Err(EditError::NotPresent),
    }
    // Prune now-empty parents, from the deepest outward.
    for prop in chain.into_iter().rev() {
        match prop.object_value() {
            Some(child) if child.properties().is_empty() => prop.remove(),
            _ => break,
        }
    }
    Ok(root.to_string())
}

fn remove_entry_toml(text: &str, spec: &AgentSpec) -> Result<String, EditError> {
    let mut doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|e: toml_edit::TomlError| EditError::Unparseable {
                format: format_name(spec),
                reason: e.to_string(),
            })?;
    let key = spec.entry.servers_key[0];
    let removed = doc
        .as_table_mut()
        .get_mut(key)
        .and_then(|i| i.as_table_like_mut())
        .map(|t| t.remove("codegraph").is_some())
        .unwrap_or(false);
    if !removed {
        return Err(EditError::NotPresent);
    }
    // Prune the parent table if it is now empty.
    let prune = doc
        .as_table_mut()
        .get(key)
        .and_then(|i| i.as_table_like())
        .map(|t| t.is_empty())
        .unwrap_or(false);
    if prune {
        doc.as_table_mut().remove(key);
    }
    Ok(doc.to_string())
}
