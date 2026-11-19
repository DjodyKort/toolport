//! Changing single fields of a SKILL.md frontmatter without rewriting the rest of it: the lines of
//! the fields that change are replaced, so comments, quoting and key spelling elsewhere stay.

use serde_json::{Map, Value as Json};
use serde_yaml::{Mapping, Value as Yaml};

/// The result of a patch: the new frontmatter text and the keys whose value is not what it was.
#[derive(Debug, PartialEq, Eq)]
pub struct Patched {
    pub yaml: String,
    pub changed: Vec<String>,
    /// True when the frontmatter could not be edited line by line (flow style, quoted keys,
    /// repeated keys) and was written out again as a whole.
    pub rewritten: bool,
}

/// A key as the YAML spells it, with its lines.
struct Block {
    key: String,
    start: usize,
    end: usize,
}

fn same_key(a: &str, b: &str) -> bool {
    a.replace('-', "_") == b.replace('-', "_")
}

fn key_of(line: &str) -> Option<String> {
    let first = line.chars().next()?;
    if first.is_whitespace()
        || matches!(
            first,
            '#' | '-'
                | '{'
                | '['
                | '"'
                | '\''
                | '?'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '%'
                | '@'
                | '`'
        )
    {
        return None;
    }
    let (key, rest) = line.split_once(':')?;
    if !(rest.is_empty() || rest.starts_with([' ', '\t', '\n', '\r'])) || key.is_empty() {
        return None;
    }
    Some(key.trim_end().to_string())
}

/// The top-level fields with their lines, or `None` when the text is not a plain block mapping.
fn blocks(lines: &[&str]) -> Option<Vec<Block>> {
    let mut found: Vec<Block> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match key_of(line) {
            Some(key) => found.push(Block {
                key,
                start: index,
                end: index + 1,
            }),
            None if line.starts_with([' ', '\t']) && !found.is_empty() => {}
            None => return None,
        }
    }
    for at in 0..found.len() {
        let limit = found.get(at + 1).map_or(lines.len(), |next| next.start);
        let mut end = limit;
        while end > found[at].start + 1 {
            let tail = lines[end - 1].trim_end();
            if tail.is_empty() || tail.starts_with('#') {
                end -= 1;
            } else {
                break;
            }
        }
        found[at].end = end;
    }
    let mut keys: Vec<String> = found.iter().map(|b| b.key.replace('-', "_")).collect();
    keys.sort();
    keys.dedup();
    (keys.len() == found.len()).then_some(found)
}

fn mapping_of(text: &str) -> Result<Mapping, String> {
    match serde_yaml::from_str::<Yaml>(text).map_err(|e| format!("invalid YAML: {e}"))? {
        Yaml::Mapping(map) => Ok(map),
        Yaml::Null => Ok(Mapping::new()),
        _ => Err("frontmatter must be a mapping".into()),
    }
}

fn lookup<'a>(map: &'a Mapping, key: &str) -> Option<(&'a Yaml, &'a Yaml)> {
    map.iter()
        .find(|(k, _)| k.as_str().is_some_and(|k| same_key(k, key)))
}

fn text_of(key: &str, value: &Yaml) -> Result<String, String> {
    let mut one = Mapping::new();
    one.insert(Yaml::String(key.to_string()), value.clone());
    serde_yaml::to_string(&one).map_err(|e| e.to_string())
}

/// Applies `patch` to the frontmatter text. A `null` removes the field. A field whose value is
/// already the one given is left as it is.
pub fn patch_frontmatter(yaml: &str, patch: &Map<String, Json>) -> Result<Patched, String> {
    let before = mapping_of(yaml)?;
    let mut wanted: Vec<(String, Option<Yaml>)> = Vec::new();
    for (key, value) in patch {
        let value = match value {
            Json::Null => None,
            other => Some(serde_yaml::to_value(other).map_err(|e| e.to_string())?),
        };
        let current = lookup(&before, key);
        let skip = match (&value, current) {
            (None, None) => true,
            (Some(new), Some((_, old))) => new == old,
            _ => false,
        };
        if !skip {
            wanted.push((key.clone(), value));
        }
    }

    let lines: Vec<&str> = yaml.split_inclusive('\n').collect();
    let found = blocks(&lines);
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_string()).collect();
    let mut rewritten = found.is_none();
    let mut after = before.clone();

    for (key, value) in &wanted {
        let spelling = lookup(&before, key)
            .and_then(|(k, _)| k.as_str().map(str::to_string))
            .unwrap_or_else(|| key.clone());
        let existing = after
            .iter()
            .find(|(k, _)| k.as_str().is_some_and(|k| same_key(k, key)))
            .map(|(k, _)| k.clone());
        match (&value, &existing) {
            (Some(new), Some(k)) => {
                after.insert(k.clone(), new.clone());
            }
            (Some(new), None) => {
                after.insert(Yaml::String(spelling.clone()), new.clone());
            }
            (None, Some(k)) => {
                after.remove(k);
            }
            (None, None) => {}
        }
    }

    let text = if let (Some(found), false) = (&found, rewritten) {
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        let mut appended = String::new();
        for (key, value) in &wanted {
            let block = found.iter().find(|b| same_key(&b.key, key));
            match (value, block) {
                (Some(new), Some(b)) => edits.push((b.start, b.end, text_of(&b.key, new)?)),
                (None, Some(b)) => edits.push((b.start, b.end, String::new())),
                (Some(new), None) => appended.push_str(&text_of(key, new)?),
                (None, None) => {}
            }
        }
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (start, end, replacement) in edits {
            out.splice(start..end, [replacement]);
        }
        let mut joined = out.concat();
        if !appended.is_empty() {
            if !joined.is_empty() && !joined.ends_with('\n') {
                joined.push('\n');
            }
            joined.push_str(&appended);
        }
        joined
    } else {
        rewritten = true;
        serde_yaml::to_string(&after).map_err(|e| e.to_string())?
    };

    let reparsed = mapping_of(&text)?;
    if reparsed != after {
        rewritten = true;
        let whole = serde_yaml::to_string(&after).map_err(|e| e.to_string())?;
        return Ok(Patched {
            changed: wanted.iter().map(|(k, _)| k.clone()).collect(),
            yaml: whole,
            rewritten,
        });
    }
    Ok(Patched {
        yaml: text,
        changed: wanted.into_iter().map(|(k, _)| k).collect(),
        rewritten,
    })
}

/// The current fields as JSON, with the keys spelled as the file spells them.
pub fn fields_of(yaml: &str) -> Result<Json, String> {
    let map = mapping_of(yaml)?;
    serde_json::to_value(&map).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn patch(yaml: &str, change: Json) -> Patched {
        patch_frontmatter(yaml, change.as_object().unwrap()).unwrap()
    }

    const SOURCE: &str = "name: nightly\n# what it does\ndescription: >-\n  Runs the nightly checks\n  and reports.\nallowed-tools: Bash(git *)\n\n# kept\nlicense: MIT\n";

    #[test]
    fn test01_only_the_lines_of_a_changed_field_are_replaced() {
        let out = patch(SOURCE, json!({"description": "Short"}));
        assert_eq!(
            out.yaml,
            "name: nightly\n# what it does\ndescription: Short\nallowed-tools: Bash(git *)\n\n# kept\nlicense: MIT\n"
        );
        assert_eq!(out.changed, ["description"]);
        assert!(!out.rewritten);
    }

    #[test]
    fn test02_a_hyphenated_key_keeps_its_spelling_when_the_patch_uses_an_underscore() {
        let out = patch(SOURCE, json!({"allowed_tools": "Read"}));
        assert!(out.yaml.contains("allowed-tools: Read\n"), "{}", out.yaml);
        assert!(!out.yaml.contains("allowed_tools"));
    }

    #[test]
    fn test03_a_new_field_is_appended_and_null_removes_one() {
        let out = patch(SOURCE, json!({"when_to_use": "At night", "license": null}));
        assert!(
            out.yaml.ends_with("# kept\nwhen_to_use: At night\n"),
            "{}",
            out.yaml
        );
        assert!(!out.yaml.contains("license"));
        assert!(out.yaml.contains("# what it does\n"));
    }

    #[test]
    fn test04_a_value_that_is_already_there_changes_nothing() {
        let out = patch(SOURCE, json!({"license": "MIT", "missing": null}));
        assert_eq!(out.yaml, SOURCE);
        assert!(out.changed.is_empty());
    }

    #[test]
    fn test05_the_value_of_a_folded_field_is_compared_by_text_not_by_layout() {
        let out = patch(
            SOURCE,
            json!({"description": "Runs the nightly checks and reports."}),
        );
        assert!(out.changed.is_empty(), "{:?}", out.changed);
    }

    #[test]
    fn test06_a_flow_style_mapping_is_written_out_again_as_a_whole() {
        let out = patch("{name: a, description: b}\n", json!({"description": "c"}));
        assert!(out.rewritten);
        assert_eq!(mapping_of(&out.yaml).unwrap().len(), 2);
        assert!(out.yaml.contains("description: c"));
    }

    #[test]
    fn test07_special_characters_are_quoted_so_the_field_reads_back_the_same() {
        let value = "Use when: asked #1, \"quoted\"\nsecond line";
        let out = patch(SOURCE, json!({"description": value}));
        let back = mapping_of(&out.yaml).unwrap();
        assert_eq!(back.get("description").and_then(Yaml::as_str), Some(value));
        assert!(out.yaml.contains("# kept"));
    }

    #[test]
    fn test08_numbers_and_booleans_keep_their_type() {
        let out = patch(
            SOURCE,
            json!({"priority": 3, "disable-model-invocation": true}),
        );
        let back = mapping_of(&out.yaml).unwrap();
        assert_eq!(back.get("priority").and_then(Yaml::as_i64), Some(3));
        assert_eq!(
            back.get("disable-model-invocation").and_then(Yaml::as_bool),
            Some(true)
        );
    }

    #[test]
    fn test09_the_fields_read_back_with_the_spelling_of_the_file() {
        let fields = fields_of(SOURCE).unwrap();
        assert_eq!(fields["allowed-tools"], "Bash(git *)");
        assert_eq!(
            fields["description"],
            "Runs the nightly checks and reports."
        );
    }
}
