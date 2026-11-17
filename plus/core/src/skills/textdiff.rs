//! A unified diff of two texts for the Compare dialog. It trims the common head and tail and
//! runs a longest-common-subsequence over what is left; a middle too large for that table is
//! reported as one replaced block.

const CONTEXT: usize = 3;
const MAX_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit<'a> {
    Same(&'a str),
    Del(&'a str),
    Add(&'a str),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub text: String,
    pub added: usize,
    pub removed: usize,
}

fn middle<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Edit<'a>> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_CELLS {
        return a
            .iter()
            .map(|l| Edit::Del(l))
            .chain(b.iter().map(|l| Edit::Add(l)))
            .collect();
    }
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
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(n + m);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(Edit::Same(a[i]));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            out.push(Edit::Del(a[i]));
            i += 1;
        } else {
            out.push(Edit::Add(b[j]));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| Edit::Del(l)));
    out.extend(b[j..].iter().map(|l| Edit::Add(l)));
    out
}

fn edits<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Edit<'a>> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut out: Vec<Edit> = a[..head].iter().map(|l| Edit::Same(l)).collect();
    out.extend(middle(&a[head..a.len() - tail], &b[head..b.len() - tail]));
    out.extend(a[a.len() - tail..].iter().map(|l| Edit::Same(l)));
    out
}

fn range(start: usize, len: usize) -> String {
    let first = if len == 0 { start } else { start + 1 };
    format!("{first},{len}")
}

/// The unified diff that turns `old` into `new`, three lines of context, empty when the texts
/// have the same lines.
pub fn unified(old: &str, new: &str, old_name: &str, new_name: &str) -> Diff {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let all = edits(&a, &b);
    let changed: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, e)| !matches!(e, Edit::Same(_)))
        .map(|(i, _)| i)
        .collect();
    let added = all.iter().filter(|e| matches!(e, Edit::Add(_))).count();
    let removed = all.iter().filter(|e| matches!(e, Edit::Del(_))).count();
    if changed.is_empty() {
        return Diff::default();
    }
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for &index in &changed {
        let low = index.saturating_sub(CONTEXT);
        let high = (index + CONTEXT).min(all.len() - 1);
        match groups.last_mut() {
            Some(last) if low <= last.1 + 1 => last.1 = last.1.max(high),
            _ => groups.push((low, high)),
        }
    }
    let mut before = vec![(0usize, 0usize); all.len() + 1];
    for (i, edit) in all.iter().enumerate() {
        let (x, y) = before[i];
        before[i + 1] = match edit {
            Edit::Same(_) => (x + 1, y + 1),
            Edit::Del(_) => (x + 1, y),
            Edit::Add(_) => (x, y + 1),
        };
    }
    let mut text = format!("--- {old_name}\n+++ {new_name}\n");
    for (low, high) in groups {
        let slice = &all[low..=high];
        let old_len = slice.iter().filter(|e| !matches!(e, Edit::Add(_))).count();
        let new_len = slice.iter().filter(|e| !matches!(e, Edit::Del(_))).count();
        text.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(before[low].0, old_len),
            range(before[low].1, new_len)
        ));
        for edit in slice {
            let (mark, line) = match edit {
                Edit::Same(l) => (' ', l),
                Edit::Del(l) => ('-', l),
                Edit::Add(l) => ('+', l),
            };
            text.push(mark);
            text.push_str(line);
            text.push('\n');
        }
    }
    Diff {
        text,
        added,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test01_same_lines_give_an_empty_diff() {
        assert_eq!(unified("a\nb\n", "a\nb\n", "x", "y"), Diff::default());
    }

    #[test]
    fn test02_a_changed_line_is_one_hunk_with_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
        let diff = unified(old, new, "library", "installed");
        assert_eq!(
            diff.text,
            "--- library\n+++ installed\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"
        );
        assert_eq!((diff.added, diff.removed), (1, 1));
    }

    #[test]
    fn test03_distant_changes_make_two_hunks() {
        let old: Vec<String> = (1..=30).map(|n| n.to_string()).collect();
        let mut new = old.clone();
        new[1] = "two".into();
        new[27] = "twenty-eight".into();
        let diff = unified(&old.join("\n"), &new.join("\n"), "a", "b");
        assert_eq!(diff.text.matches("@@ -").count(), 2);
    }

    #[test]
    fn test04_added_lines_at_the_end_and_an_empty_old_text() {
        let diff = unified("", "a\nb\n", "a", "b");
        assert_eq!(diff.text, "--- a\n+++ b\n@@ -0,0 +1,2 @@\n+a\n+b\n");
        assert_eq!((diff.added, diff.removed), (2, 0));
    }

    #[test]
    fn test05_a_middle_too_large_for_the_table_is_one_replaced_block() {
        let old: Vec<String> = (0..3000).map(|n| format!("old {n}")).collect();
        let new: Vec<String> = (0..3000).map(|n| format!("new {n}")).collect();
        let diff = unified(&old.join("\n"), &new.join("\n"), "a", "b");
        assert_eq!((diff.added, diff.removed), (3000, 3000));
    }
}
