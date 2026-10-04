use super::frontmatter::frontmatter_accepted;
use super::scalar;
use crate::plus::randutil::run_cases;
use serde_yaml::Value;

fn file(yaml: &str) -> String {
    format!("---\n{yaml}---\n\nBody\n")
}

fn read(yaml: &str) -> Option<Value> {
    serde_yaml::from_str(yaml).ok()
}

fn value_of(scalar: &str) -> Option<String> {
    match read(&format!("k: {scalar}\n"))? {
        Value::Mapping(map) if map.len() == 1 => map.get("k")?.as_str().map(str::to_string),
        _ => None,
    }
}

fn item_of(scalar: &str) -> Option<String> {
    match read(&format!("k: [{scalar}, tail]\n"))? {
        Value::Mapping(map) => match map.get("k")? {
            Value::Sequence(items) if items.len() == 2 => items[0].as_str().map(str::to_string),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn quoted_keeps_mcpm_bytes_for_one_line_text_that_reads_back() {
    for text in [
        "Review a diff for correctness",
        "Use when: the user asks",
        "  padded  ",
        "multi-byte é日🙂",
        "a # not a comment",
        "",
    ] {
        assert_eq!(scalar::quoted(text), format!("\"{text}\""), "{text:?}");
    }
}

#[test]
fn quoted_falls_back_when_the_verbatim_form_would_not_read_back() {
    for (text, want) in [
        ("say \"hi\"", "'say \"hi\"'"),
        ("a\\b", "'a\\b'"),
        ("it's \"x\"", "'it''s \"x\"'"),
        ("trailing\\", "'trailing\\'"),
    ] {
        assert_eq!(scalar::quoted(text), want, "{text:?}");
        assert_eq!(value_of(want).as_deref(), Some(text));
    }
}

#[test]
fn multi_line_text_becomes_an_indented_block_literal() {
    for (text, want) in [
        ("a\nb", "|-\n  a\n  b"),
        ("a\n\nb", "|-\n  a\n\n  b"),
        ("a\nb\n", "|\n  a\n  b"),
        ("say \"hi\"\nit's\n\nok", "|-\n  say \"hi\"\n  it's\n\n  ok"),
    ] {
        assert_eq!(scalar::quoted(text), want, "{text:?}");
        assert_eq!(value_of(want).as_deref(), Some(text));
    }
}

#[test]
fn multi_line_text_without_a_faithful_block_form_is_escaped_on_one_line() {
    for (text, want) in [
        (" a\nb", "\" a\\nb\""),
        ("a \nb", "\"a \\nb\""),
        ("a\nb\n\n", "\"a\\nb\\n\\n\""),
        ("a\r\nb", "\"a\\r\\nb\""),
        ("\na", "\"\\na\""),
        ("a\u{85}b", "\"a\\Nb\""),
    ] {
        let got = scalar::quoted(text);
        assert_eq!(got, want, "{text:?}");
        assert_eq!(value_of(&got).as_deref(), Some(text));
    }
}

#[test]
fn flow_list_items_never_use_a_block() {
    assert_eq!(scalar::quoted_item("**/*.py"), "\"**/*.py\"");
    assert_eq!(scalar::quoted_item("a\nb"), "\"a\\nb\"");
    assert_eq!(scalar::quoted_item("say \"hi\""), "'say \"hi\"'");
    assert_eq!(
        item_of(&scalar::quoted_item("a, b]")).as_deref(),
        Some("a, b]")
    );
}

#[test]
fn plain_text_is_kept_while_a_strict_parser_reads_it_back() {
    for text in [
        "Read, Grep, Bash(git diff:*)",
        "Read",
        "a/b/c.py",
        "mixed é日",
    ] {
        assert_eq!(scalar::plain(text), text, "{text:?}");
    }
}

#[test]
fn plain_text_a_strict_parser_would_not_read_back_is_quoted() {
    for (text, want) in [
        ("**/*.py", "\"**/*.py\""),
        ("a: b", "\"a: b\""),
        ("# note", "\"# note\""),
        ("- x", "\"- x\""),
        ("[a, b", "\"[a, b\""),
        ("a\nb", "|-\n  a\n  b"),
        ("", "\"\""),
        ("true", "\"true\""),
        ("12", "\"12\""),
    ] {
        let got = scalar::plain(text);
        assert_eq!(got, want, "{text:?}");
        assert_eq!(value_of(&got).as_deref(), Some(text));
    }
}

#[test]
fn loose_text_is_only_changed_when_it_spans_lines() {
    assert_eq!(scalar::loose("**/*.py"), "**/*.py");
    assert_eq!(scalar::loose("*.rs, src/**"), "*.rs, src/**");
    assert_eq!(scalar::loose("a\nb"), "|-\n  a\n  b");
    assert_eq!(scalar::loose(" a\nb"), "\" a\\nb\"");
}

#[test]
fn every_scalar_form_reads_back_the_value_it_was_made_from() {
    run_cases("skills-scalar-reads-back", 4000, |_, rng| {
        let text = rng.garbage(40);
        for (name, form) in [
            ("quoted", scalar::quoted(&text)),
            ("plain", scalar::plain(&text)),
            ("loose", scalar::loose(&text)),
        ] {
            if name == "loose" && !text.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) {
                assert_eq!(form, text);
                continue;
            }
            assert_eq!(
                value_of(&form).as_deref(),
                Some(text.as_str()),
                "{name}: {form:?}"
            );
        }
        let item = scalar::quoted_item(&text);
        assert_eq!(item_of(&item).as_deref(), Some(text.as_str()), "{item:?}");
    });
}

#[test]
fn frontmatter_made_of_the_scalar_forms_is_accepted() {
    run_cases("skills-scalar-accepted", 2000, |_, rng| {
        let text = rng.garbage(60).replace("---", "-_-");
        let yaml = format!(
            "name: n\ndescription: {}\nallowed-tools: {}\npaths: [{}]\nnext: v\n",
            scalar::quoted(&text),
            scalar::plain(&text),
            scalar::quoted_item(&text)
        );
        let doc = file(&yaml);
        assert_eq!(frontmatter_accepted(&doc), Ok(()), "{doc:?}");
    });
}
