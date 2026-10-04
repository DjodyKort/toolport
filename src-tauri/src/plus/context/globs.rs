//! Path globs the way Claude Code matches `claudeMdExcludes`: `*` and `?` stay inside one path
//! segment, `**` crosses segments, `[abc]` is a class and `{a,b}` an alternation. Everything else
//! is literal, so a plain path still matches only itself.

use regex::Regex;

fn translate(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from("^");
    let mut braces = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' => {
                let mut stars = 1;
                while chars.get(i + stars) == Some(&'*') {
                    stars += 1;
                }
                i += stars - 1;
                if stars >= 2 {
                    if chars.get(i + 1) == Some(&'/') {
                        out.push_str("(?:.*/)?");
                        i += 1;
                    } else {
                        out.push_str(".*");
                    }
                } else {
                    out.push_str("[^/]*");
                }
            }
            '?' => out.push_str("[^/]"),
            '[' => match chars[i + 1..].iter().position(|x| *x == ']') {
                Some(end) if end > 0 => {
                    let body: String = chars[i + 1..i + 1 + end].iter().collect();
                    let body = match body.strip_prefix('!') {
                        Some(rest) => format!("^{}", rest.replace('\\', "\\\\")),
                        None => body.replace('\\', "\\\\"),
                    };
                    out.push('[');
                    out.push_str(&body.replace('[', "\\["));
                    out.push(']');
                    i += end + 1;
                }
                _ => out.push_str("\\["),
            },
            '{' if chars[i + 1..].contains(&'}') => {
                braces += 1;
                out.push_str("(?:");
            }
            ',' if braces > 0 => out.push('|'),
            '}' if braces > 0 => {
                braces -= 1;
                out.push(')');
            }
            other => out.push_str(&regex::escape(&other.to_string())),
        }
        i += 1;
    }
    for _ in 0..braces {
        out.push(')');
    }
    out.push('$');
    out
}

pub fn glob_match(pattern: &str, text: &str) -> bool {
    if !pattern.contains(['*', '?', '[', '{']) {
        return pattern == text;
    }
    Regex::new(&translate(pattern)).is_ok_and(|re| re.is_match(text))
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn a_plain_path_matches_only_itself() {
        assert!(glob_match("/a/b/CLAUDE.md", "/a/b/CLAUDE.md"));
        assert!(!glob_match("/a/b/CLAUDE.md", "/a/b/CLAUDE.md.bak"));
    }

    #[test]
    fn double_star_crosses_directories_and_a_single_star_does_not() {
        assert!(glob_match("**/parent/CLAUDE.md", "/home/u/work/parent/CLAUDE.md"));
        assert!(glob_match("/home/**/CLAUDE.md", "/home/CLAUDE.md"));
        assert!(glob_match("/home/**/CLAUDE.md", "/home/a/b/CLAUDE.md"));
        assert!(glob_match("/home/*/CLAUDE.md", "/home/a/CLAUDE.md"));
        assert!(!glob_match("/home/*/CLAUDE.md", "/home/a/b/CLAUDE.md"));
        assert!(!glob_match("**/parent/CLAUDE.md", "/home/u/work/other/CLAUDE.md"));
    }

    #[test]
    fn classes_alternations_and_question_marks_work() {
        assert!(glob_match("/x/client-[ab]/CLAUDE.md", "/x/client-a/CLAUDE.md"));
        assert!(!glob_match("/x/client-[ab]/CLAUDE.md", "/x/client-c/CLAUDE.md"));
        assert!(glob_match("/x/{one,two}/CLAUDE.md", "/x/two/CLAUDE.md"));
        assert!(!glob_match("/x/{one,two}/CLAUDE.md", "/x/three/CLAUDE.md"));
        assert!(glob_match("/x/c?/CLAUDE.md", "/x/c1/CLAUDE.md"));
        assert!(!glob_match("/x/c?/CLAUDE.md", "/x/c12/CLAUDE.md"));
    }

    #[test]
    fn regex_metacharacters_in_a_pattern_stay_literal() {
        assert!(glob_match("/a.b/+c/*.md", "/a.b/+c/x.md"));
        assert!(!glob_match("/a.b/+c/*.md", "/aXb/+c/x.md"));
    }
}
