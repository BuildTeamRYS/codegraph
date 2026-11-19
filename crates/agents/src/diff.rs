//! A tiny line diff for showing exactly what would change.
//! LCS-based; O(n·m) — fine for config files (a few dozen lines).
//! Merged hunks get two lines of context on each side.

enum Op {
    Keep(usize),
    Del(usize),
    Ins(usize),
}

pub fn line_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());

    // dp[i][j] = LCS length of a[i..], b[j..].
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut ops: Vec<Op> = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Keep(i));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Del(i));
            i += 1;
        } else {
            ops.push(Op::Ins(j));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(i));
        i += 1;
    }
    while j < m {
        ops.push(Op::Ins(j));
        j += 1;
    }

    const CONTEXT: usize = 2;
    // Merge change indices extended by CONTEXT on both sides.
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, Op::Keep(_)))
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return String::new();
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for c in changed {
        let start = c.saturating_sub(CONTEXT);
        let end = (c + CONTEXT + 1).min(ops.len());
        match ranges.last_mut() {
            Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
            _ => ranges.push((start, end)),
        }
    }

    let mut out = String::new();
    for range in ranges {
        for op in &ops[range.0..range.1] {
            match op {
                Op::Keep(ai) => {
                    out.push_str("  ");
                    out.push_str(a[*ai]);
                    out.push('\n');
                }
                Op::Del(ai) => {
                    out.push_str("- ");
                    out.push_str(a[*ai]);
                    out.push('\n');
                }
                Op::Ins(bj) => {
                    out.push_str("+ ");
                    out.push_str(b[*bj]);
                    out.push('\n');
                }
            }
        }
        out.push_str("···\n");
    }
    out.truncate(out.len().saturating_sub("···\n".len()));
    out
}
