//! The one line Toolport adds to the shell startup file, and nothing else in it. The line carries
//! a marker, so adding it twice changes nothing and removing it touches no other byte. Text is
//! handled as lines that keep their own line endings.

use std::path::Path;

pub const MARKER: &str = "toolport:context-shims";

/// Files whose own `claude()` must be defined before ours, or ours is replaced by theirs.
pub const MUST_PRECEDE: [&str; 2] = ["shell-wrapper.sh", "compression-shims.zsh"];

const SHIMS_NAME: &str = "context-shims.zsh";

fn double_quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The line to add. A file inside the home folder is named from `$HOME`, so the line survives a
/// renamed account; the guard keeps a shell that starts without Toolport quiet.
pub fn source_line(home: &Path, shims: &Path) -> String {
    let path = match shims.strip_prefix(home) {
        Ok(rest) => format!("\"$HOME/{}\"", double_quoted(&rest.to_string_lossy())),
        Err(_) => format!("\"{}\"", double_quoted(&shims.to_string_lossy())),
    };
    format!("[ -r {path} ] && source {path}  # {MARKER}")
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

fn is_ours(line: &str) -> bool {
    !is_comment(line) && line.contains(MARKER)
}

fn sources_shims(line: &str) -> bool {
    !is_comment(line) && (line.contains(MARKER) || line.contains(SHIMS_NAME))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Lines carrying the marker, as 1-based numbers.
    pub marked: Vec<usize>,
    /// Lines that source the shims file without the marker (added by hand).
    pub by_hand: Vec<usize>,
    /// Files of `MUST_PRECEDE` that are sourced after the last line of ours.
    pub sourced_after: Vec<&'static str>,
}

impl Scan {
    pub fn sourced(&self) -> bool {
        !self.marked.is_empty() || !self.by_hand.is_empty()
    }

    pub fn line(&self) -> Option<usize> {
        self.marked.iter().chain(&self.by_hand).copied().max()
    }
}

pub fn scan(text: &str) -> Scan {
    let mut out = Scan::default();
    for (index, line) in text.lines().enumerate() {
        if is_ours(line) {
            out.marked.push(index + 1);
        } else if sources_shims(line) {
            out.by_hand.push(index + 1);
        }
    }
    if let Some(last) = out.line() {
        for name in MUST_PRECEDE {
            let later = text
                .lines()
                .enumerate()
                .any(|(index, line)| index + 1 > last && !is_comment(line) && line.contains(name));
            if later {
                out.sourced_after.push(name);
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Our line was not there and is now the last line.
    Added,
    /// Our line was there and is now the last line.
    Moved,
    /// Nothing to change.
    Unchanged,
}

fn without_ours(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| !is_ours(line))
        .collect()
}

/// The text with our line as its last line. Returns whether a newline had to be added after the
/// previous last line, which the undo needs to take away again.
pub fn wire(text: &str, line: &str) -> (String, Edit, bool) {
    let found = scan(text);
    if !found.by_hand.is_empty() && found.marked.is_empty() {
        return (text.to_string(), Edit::Unchanged, false);
    }
    let wanted_last = text
        .lines()
        .last()
        .is_some_and(|last| last == line && found.marked.len() == 1);
    if wanted_last && found.sourced_after.is_empty() {
        return (text.to_string(), Edit::Unchanged, false);
    }
    let edit = if found.marked.is_empty() {
        Edit::Added
    } else {
        Edit::Moved
    };
    let mut kept = without_ours(text);
    let added_newline = !kept.is_empty() && !kept.ends_with('\n');
    if added_newline {
        kept.push('\n');
    }
    kept.push_str(line);
    kept.push('\n');
    (kept, edit, added_newline)
}

/// The text without our line. When our line was the last one and `added_newline` says that
/// adding it also added a newline, that newline goes too, so the file is as it was.
pub fn unwire(text: &str, added_newline: bool) -> (String, bool) {
    let removed = text.lines().any(is_ours);
    if !removed {
        return (text.to_string(), false);
    }
    let last_was_ours = text.lines().last().is_some_and(is_ours);
    let mut kept = without_ours(text);
    if last_was_ours && added_newline && kept.ends_with('\n') {
        kept.pop();
    }
    (kept, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::testkit::run_cases;
    use std::path::PathBuf;

    fn line() -> String {
        source_line(
            &PathBuf::from("/Users/me"),
            &PathBuf::from("/Users/me/Library/Application Support/Toolport/context-shims.zsh"),
        )
    }

    #[test]
    fn test01_the_line_names_the_file_from_home_and_carries_the_marker() {
        assert_eq!(
            line(),
            "[ -r \"$HOME/Library/Application Support/Toolport/context-shims.zsh\" ] && source \"$HOME/Library/Application Support/Toolport/context-shims.zsh\"  # toolport:context-shims"
        );
        let outside = source_line(
            &PathBuf::from("/Users/me"),
            &PathBuf::from("/opt/a\"b$c/context-shims.zsh"),
        );
        assert!(
            outside.contains("\"/opt/a\\\"b\\$c/context-shims.zsh\""),
            "{outside}"
        );
    }

    #[test]
    fn test02_wiring_twice_changes_nothing_the_second_time() {
        let (once, edit, _) = wire("export A=1\n", &line());
        assert_eq!(edit, Edit::Added);
        let (twice, edit, _) = wire(&once, &line());
        assert_eq!(edit, Edit::Unchanged);
        assert_eq!(once, twice);
    }

    #[test]
    fn test03_the_line_goes_last_and_a_file_without_a_final_newline_gets_one() {
        let (text, _, added) = wire("alias a=b", &line());
        assert!(added);
        assert_eq!(text, format!("alias a=b\n{}\n", line()));
        let (empty, _, added) = wire("", &line());
        assert!(!added);
        assert_eq!(empty, format!("{}\n", line()));
    }

    #[test]
    fn test04_our_line_before_the_other_wrapper_is_moved_below_it() {
        let text = format!("{}\nsource ~/tools/shell-wrapper.sh\nexport B=2\n", line());
        assert_eq!(scan(&text).sourced_after, ["shell-wrapper.sh"]);
        let (fixed, edit, _) = wire(&text, &line());
        assert_eq!(edit, Edit::Moved);
        assert_eq!(
            fixed,
            format!("source ~/tools/shell-wrapper.sh\nexport B=2\n{}\n", line())
        );
        assert!(scan(&fixed).sourced_after.is_empty());
    }

    #[test]
    fn test05_a_line_added_by_hand_is_left_alone_and_a_commented_one_is_ignored() {
        let by_hand = "source ~/x/context-shims.zsh\n";
        let (same, edit, _) = wire(by_hand, &line());
        assert_eq!((same.as_str(), edit), (by_hand, Edit::Unchanged));
        assert_eq!(scan(by_hand).by_hand, [1]);
        let commented = format!("# {}\n", line());
        let found = scan(&commented);
        assert!(!found.sourced(), "{found:?}");
        let (added, edit, _) = wire(&commented, &line());
        assert_eq!(edit, Edit::Added);
        assert!(added.starts_with(&commented));
    }

    #[test]
    fn test06_unwire_removes_only_our_line_and_the_newline_it_added() {
        let (text, _, added) = wire("alias a=b", &line());
        assert_eq!(unwire(&text, added), ("alias a=b".to_string(), true));
        let (text, _, added) = wire("alias a=b\n", &line());
        assert_eq!(unwire(&text, added), ("alias a=b\n".to_string(), true));
        assert_eq!(unwire("x\n", false), ("x\n".to_string(), false));
    }

    #[test]
    fn test07_unwire_keeps_what_was_written_after_our_line() {
        let text = format!("a\n{}\nlater\n", line());
        assert_eq!(unwire(&text, true), ("a\nlater\n".to_string(), true));
    }

    #[test]
    fn test08_wire_then_unwire_gives_back_every_file_byte_for_byte() {
        run_cases("shell_rc_round_trip", 300, |_, rng| {
            let mut original = rng.garbage(200);
            original.retain(|c| c != '\u{0}');
            let original = original.replace(MARKER, "x");
            let (wired, _, added) = wire(&original, &line());
            let (back, removed) = unwire(&wired, added);
            assert!(removed, "{original:?}");
            assert_eq!(back, original, "{original:?}");
            let (again, edit, _) = wire(&wired, &line());
            assert_eq!(edit, Edit::Unchanged);
            assert_eq!(again, wired);
        });
    }
}
