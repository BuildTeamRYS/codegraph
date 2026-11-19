//! The marked instruction block (`<!-- codegraph:start -->` …
//! `<!-- codegraph:end -->`) written into an agent's instruction file.
//!
//! Idempotent: re-running replaces the marked block in place; everything
//! outside the markers is untouched. Removal strips the block and any
//! blank lines left behind.

pub const START: &str = "<!-- codegraph:start -->";
pub const END: &str = "<!-- codegraph:end -->";

/// The block content. Keep it between 5 and 8 lines (PLAN.md 2.5C).
pub fn block() -> String {
    format!(
        "{START}\n\
This repo has a codegraph index (.codegraph/index.db). Use it before opening many files:\n\
- Locate symbols, callers/callees and impact with the `codegraph` MCP tools (`find_symbol`, `outline`, `callers`, `callees`, `get_context`), or `codegraph query <tool>` from a shell.\n\
- Read only the `file:start-end` ranges results return; edges carry `[certain|inferred|unknown]` confidence.\n\
- If the header says `index@<commit> stale` or a result is `unknown`, fall back to normal search.\n\
- After large refactors, run `codegraph index` to refresh the store.\n\
{END}"
    )
}

/// Insert or replace the marked block in `text`. Returns the new text.
pub fn upsert(text: &str) -> String {
    match (text.find(START), text.find(END)) {
        (Some(s), Some(e)) if e >= s => {
            let after = e + END.len();
            // Swallow a trailing newline right after the end marker, if any,
            // so repeated upserts never accumulate blank lines.
            let after = if text[after..].starts_with('\n') {
                after + 1
            } else {
                after
            };
            let before = text[..s].trim_end();
            let rest = &text[after..];
            let mut out = String::with_capacity(text.len() + 16);
            if !before.is_empty() {
                out.push_str(before);
                out.push('\n');
            }
            out.push_str(&block());
            out.push('\n');
            if !rest.trim().is_empty() {
                out.push_str(rest.trim_start_matches('\n'));
            }
            out
        }
        _ => {
            let mut out = String::with_capacity(text.len() + block().len() + 2);
            let trimmed = text.trim_end();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
            out.push_str(&block());
            out.push('\n');
            out
        }
    }
}

/// Remove the marked block. Returns the new text (or `None` if absent).
pub fn strip(text: &str) -> Option<String> {
    let s = text.find(START)?;
    let e = text.find(END)?;
    if e < s {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(text[..s].trim_end());
    let rest = &text[e + END.len()..];
    let rest_trimmed = rest.trim_start_matches('\n');
    if !out.is_empty() && !rest_trimmed.is_empty() {
        out.push('\n');
    }
    out.push_str(rest_trimmed);
    if out.is_empty() || !out.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// True when the text contains the marked block.
pub fn is_present(text: &str) -> bool {
    text.contains(START) && text.contains(END)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_then_strip_roundtrip() {
        let original = "# My repo\n\nSome rules.\n";
        let with = upsert(original);
        assert!(is_present(&with));
        assert!(with.contains("# My repo"));
        let back = strip(&with).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn upsert_is_idempotent() {
        let text = upsert("# Hi\n");
        let twice = upsert(&text);
        assert_eq!(text, twice);
    }

    #[test]
    fn strip_absent_returns_none() {
        assert_eq!(strip("# nothing here\n"), None);
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let once = upsert("# Hi\n");
        let edited = once.replace("find_symbol", "FIND_SYMBOL");
        let twice = upsert(&edited);
        assert!(twice.contains("find_symbol"));
        assert!(!twice.contains("FIND_SYMBOL"));
    }
}
