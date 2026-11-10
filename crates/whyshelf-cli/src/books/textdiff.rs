//! A unified line diff of two texts, for `books diff`: small, and with no dependency.
//!
//! The lines both texts start and end with are set aside first; the rest is compared
//! with a longest common subsequence. When what is left is too large for that table
//! ([`TABLE_LIMIT`] cells), the rest is shown as removed and added whole: the
//! answer is still true, only less tight. Three lines of context are shown around
//! each change, as `diff -u` does.
//!
//! ```text
//!   --- approved/tests/a.rs
//!   +++ now/tests/a.rs
//!   @@ -1,2 +1,2 @@
//!    fn check() {
//!   -    assert!(fast);
//!   +    assert!(faster);
//! ```

/// The lines of context around a change.
const CONTEXT: usize = 3;

/// The most cells of the table of a longest common subsequence: past it, the middle
/// of the two texts is shown as removed and added whole.
pub const TABLE_LIMIT: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    Removed,
    Added,
}

/// The edit from `old` to `new` as a unified diff with these two labels; an empty
/// text when the two are the same line for line.
pub fn unified(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let ops = edit(&a, &b);
    if ops.iter().all(|op| *op == Op::Same) {
        return String::new();
    }

    // The line of each text before each op, so a hunk knows where it starts.
    let mut at = Vec::with_capacity(ops.len() + 1);
    let (mut i, mut j) = (0usize, 0usize);
    for op in &ops {
        at.push((i, j));
        match op {
            Op::Same => {
                i += 1;
                j += 1;
            }
            Op::Removed => i += 1,
            Op::Added => j += 1,
        }
    }
    at.push((i, j));

    let changes: Vec<usize> = (0..ops.len()).filter(|k| ops[*k] != Op::Same).collect();
    let mut out = format!("--- {old_label}\n+++ {new_label}\n");
    let mut group_start = 0;
    while group_start < changes.len() {
        let mut group_end = group_start;
        while group_end + 1 < changes.len()
            && changes[group_end + 1] - changes[group_end] <= 2 * CONTEXT + 1
        {
            group_end += 1;
        }
        let from = changes[group_start].saturating_sub(CONTEXT);
        let to = (changes[group_end] + 1 + CONTEXT).min(ops.len());
        let (old_from, new_from) = at[from];
        let (old_to, new_to) = at[to];
        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(old_from, old_to - old_from),
            range(new_from, new_to - new_from)
        ));
        for k in from..to {
            let (oi, ni) = at[k];
            let (mark, line) = match ops[k] {
                Op::Same => (' ', a[oi]),
                Op::Removed => ('-', a[oi]),
                Op::Added => ('+', b[ni]),
            };
            out.push(mark);
            out.push_str(line);
            out.push('\n');
        }
        group_start = group_end + 1;
    }
    out
}

/// A range of a hunk header: the first line (1-based) and the count, or, for no
/// line, the line after which the change is.
fn range(start: usize, count: usize) -> String {
    if count == 0 {
        format!("{start},0")
    } else {
        format!("{},{count}", start + 1)
    }
}

/// The ops that turn `a` into `b`.
fn edit(a: &[&str], b: &[&str]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);

    let mut ops = vec![Op::Same; prefix];
    if am.len().saturating_mul(bm.len()) <= TABLE_LIMIT {
        ops.extend(lcs(am, bm));
    } else {
        ops.extend(std::iter::repeat_n(Op::Removed, am.len()));
        ops.extend(std::iter::repeat_n(Op::Added, bm.len()));
    }
    ops.extend(std::iter::repeat_n(Op::Same, suffix));
    ops
}

/// The ops of a longest common subsequence of `a` and `b`, removals before
/// additions where both are possible.
fn lcs(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    // table[i][j]: the length of an LCS of a[i..] and b[j..].
    let width = m + 1;
    let mut table = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * width + j] = if a[i] == b[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Same);
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            ops.push(Op::Removed);
            i += 1;
        } else {
            ops.push(Op::Added);
            j += 1;
        }
    }
    ops.extend(std::iter::repeat_n(Op::Removed, n - i));
    ops.extend(std::iter::repeat_n(Op::Added, m - j));
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_has_no_diff() {
        assert_eq!(unified("a\nb\n", "a\nb\n", "x", "y"), "");
    }

    #[test]
    fn a_changed_line_is_removed_and_added_with_its_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n";
        assert_eq!(
            unified(old, new, "approved/f", "now/f"),
            "--- approved/f\n+++ now/f\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+FIVE\n 6\n 7\n 8\n"
        );
    }

    #[test]
    fn changes_far_apart_are_two_hunks_and_near_ones_are_one() {
        let text = |changed: &[usize]| -> String {
            (1..=20)
                .map(|n| {
                    if changed.contains(&n) {
                        format!("line {n} changed\n")
                    } else {
                        format!("line {n}\n")
                    }
                })
                .collect()
        };
        let old = text(&[]);
        assert_eq!(
            unified(&old, &text(&[2, 19]), "a", "b")
                .matches("@@ -")
                .count(),
            2
        );
        assert_eq!(
            unified(&old, &text(&[5, 9]), "a", "b")
                .matches("@@ -")
                .count(),
            1
        );
    }

    #[test]
    fn a_text_from_nothing_and_to_nothing_are_all_added_and_all_removed() {
        assert_eq!(
            unified("", "a\nb\n", "x", "y"),
            "--- x\n+++ y\n@@ -0,0 +1,2 @@\n+a\n+b\n"
        );
        assert_eq!(
            unified("a\n", "", "x", "y"),
            "--- x\n+++ y\n@@ -1,1 +0,0 @@\n-a\n"
        );
    }

    #[test]
    fn an_insertion_keeps_the_lines_around_it() {
        let diff = unified("a\nb\nc\n", "a\nb\nnew\nc\n", "x", "y");
        assert_eq!(diff, "--- x\n+++ y\n@@ -1,3 +1,4 @@\n a\n b\n+new\n c\n");
    }
}
