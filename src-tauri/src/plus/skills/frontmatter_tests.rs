use super::frontmatter::{
    frontmatter_accepted, output_accepted, strictness_for, Reason, Strictness,
};
use serde_yaml::Value;

const LEGACY_UNINDENTED: &str =
    include_str!("../../../tests/fixtures/skills-frontmatter/legacy-unindented.txt");
const LEGACY_INNER_QUOTES: &str =
    include_str!("../../../tests/fixtures/skills-frontmatter/legacy-inner-quotes.txt");
const LIBRARY_SINGLE_QUOTED: &str =
    include_str!("../../../tests/fixtures/skills-frontmatter/library-single-quoted.txt");

fn file(yaml: &str) -> String {
    format!("---\n{yaml}---\n\nBody\n")
}

fn rejected(text: &str) -> Reason {
    frontmatter_accepted(text).expect_err("the frontmatter should be rejected")
}

fn read(yaml: &str) -> Option<Value> {
    serde_yaml::from_str(yaml).ok()
}

#[test]
fn the_library_form_of_a_multi_line_description_is_accepted() {
    assert_eq!(frontmatter_accepted(LIBRARY_SINGLE_QUOTED), Ok(()));
}

#[test]
fn block_scalars_and_plain_values_are_accepted() {
    for yaml in [
        "name: n\ndescription: |-\n  one\n  two\n",
        "name: n\ndescription: |\n  one\n\n  two\nallowed-tools: Read, Grep\n",
        "name: n\ndescription: >-\n  folded\n  text\n",
        "name: n\ndescription: plain text\npaths:\n  - \"**/*.py\"\n  - 'src/**'\n",
    ] {
        assert_eq!(frontmatter_accepted(&file(yaml)), Ok(()), "{yaml}");
    }
}

#[test]
fn block_scalar_content_is_not_mistaken_for_frontmatter_structure() {
    let yaml =
        "name: n\ndescription: |-\n  key: \"not a value\n  - 'also not\n  other: v\nnext: ok\n";
    assert_eq!(frontmatter_accepted(&file(yaml)), Ok(()));
}

#[test]
fn a_file_without_frontmatter_has_nothing_to_reject() {
    for text in ["", "no newline", "# Title\n\nBody\n", "----\nx\n----\n"] {
        assert_eq!(frontmatter_accepted(text), Ok(()), "{text:?}");
    }
    assert_eq!(frontmatter_accepted("---\n---\n\nBody\n"), Ok(()));
}

#[test]
fn the_legacy_unindented_continuation_is_valid_yaml_that_claude_code_rejects() {
    let yaml = LEGACY_UNINDENTED
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---\n").next())
        .unwrap();
    assert!(
        read(yaml).is_some(),
        "premise: a strict YAML parse accepts this form"
    );
    assert_eq!(
        rejected(LEGACY_UNINDENTED),
        Reason::UnindentedContinuation {
            line: 4,
            text: "Use when the user says wrap up or stop for today.".into()
        }
    );
}

#[test]
fn inner_quotes_in_a_double_quoted_description_are_invalid_yaml() {
    assert!(matches!(
        rejected(LEGACY_INNER_QUOTES),
        Reason::InvalidYaml(_)
    ));
}

#[test]
fn an_unindented_single_quoted_continuation_is_rejected() {
    let text = file("name: n\ndescription: 'first\nsecond'\n");
    assert_eq!(
        rejected(&text),
        Reason::UnindentedContinuation {
            line: 4,
            text: "second'".into()
        }
    );
}

#[test]
fn a_continuation_must_be_indented_further_than_its_key() {
    let nested = file("name: n\nmeta:\n  note: \"first\n  second\"\n");
    assert!(matches!(
        rejected(&nested),
        Reason::UnindentedContinuation { line: 5, .. }
    ));
    let deeper = file("name: n\nmeta:\n  note: \"first\n    second\"\n");
    assert_eq!(frontmatter_accepted(&deeper), Ok(()));
    let item = file("name: n\npaths:\n  - \"first\n  second\"\n");
    assert!(matches!(
        rejected(&item),
        Reason::UnindentedContinuation { line: 5, .. }
    ));
}

#[test]
fn blank_lines_inside_a_quoted_value_are_not_continuations() {
    let text = file("name: n\ndescription: 'first\n\n  second'\nnext: v\n");
    assert_eq!(frontmatter_accepted(&text), Ok(()));
}

#[test]
fn a_value_that_closes_on_its_own_line_does_not_swallow_the_next_key() {
    for yaml in [
        "description: \"one line\"\nname: n\n",
        "description: 'it''s closed'\nname: n\n",
        "description: \"a \\\" b\"\nname: n\n",
        "description: \"\"\nname: n\n",
        "description: first\nname: n\n",
    ] {
        assert_eq!(frontmatter_accepted(&file(yaml)), Ok(()), "{yaml}");
    }
}

#[test]
fn an_unterminated_frontmatter_is_rejected() {
    assert_eq!(rejected("---\nname: n\n"), Reason::Unterminated);
    assert_eq!(rejected("---\n"), Reason::Unterminated);
}

#[test]
fn frontmatter_that_is_not_a_mapping_is_rejected() {
    assert_eq!(rejected(&file("- a\n- b\n")), Reason::NotAMapping);
    assert_eq!(rejected(&file("just text\n")), Reason::NotAMapping);
}

#[test]
fn duplicate_keys_and_stray_syntax_are_invalid_yaml() {
    assert!(matches!(
        rejected(&file("name: a\nname: b\n")),
        Reason::InvalidYaml(_)
    ));
    assert!(matches!(
        rejected(&file("name: a: b\n")),
        Reason::InvalidYaml(_)
    ));
    assert!(matches!(
        rejected(&file("paths: **/*.py\n")),
        Reason::InvalidYaml(_)
    ));
}

#[test]
fn crlf_files_are_judged_like_lf_files() {
    let crlf = |text: &str| text.replace('\n', "\r\n");
    assert_eq!(frontmatter_accepted(&crlf(LIBRARY_SINGLE_QUOTED)), Ok(()));
    assert!(matches!(
        rejected(&crlf(LEGACY_UNINDENTED)),
        Reason::UnindentedContinuation { line: 4, .. }
    ));
}

#[test]
fn reasons_have_stable_codes_and_readable_messages() {
    let long = "x".repeat(100);
    let cases = [
        (
            Reason::Unterminated,
            "unterminated",
            "the frontmatter has no closing `---` line".to_string(),
        ),
        (
            Reason::UnindentedContinuation {
                line: 7,
                text: long.clone(),
            },
            "unindented-continuation",
            format!(
                "line 7 continues a quoted multi-line value without indentation: {}",
                &long[..60]
            ),
        ),
        (
            Reason::InvalidYaml("boom".into()),
            "invalid-yaml",
            "invalid YAML: boom".into(),
        ),
        (
            Reason::NotAMapping,
            "not-a-mapping",
            "the frontmatter is not a YAML mapping".into(),
        ),
    ];
    for (reason, code, message) in cases {
        assert_eq!(reason.code(), code);
        assert_eq!(reason.to_string(), message);
    }
}

#[test]
fn only_clients_with_plain_yaml_frontmatter_are_held_to_a_strict_parse() {
    for key in ["claude-code", "codex-cli", "gemini-cli", "goose-cli"] {
        assert_eq!(strictness_for(key), Strictness::Strict, "{key}");
    }
    for key in [
        "cursor", "windsurf", "cline", "continue", "roo-code", "trae", "vscode", "unknown",
    ] {
        assert_eq!(strictness_for(key), Strictness::Shape, "{key}");
    }
}

#[test]
fn clients_with_their_own_glob_dialect_keep_unquoted_globs_but_not_broken_continuations() {
    let globs = file("description: \"d\"\nglobs: **/*.py\n");
    assert!(matches!(
        output_accepted("claude-code", &globs),
        Err(Reason::InvalidYaml(_))
    ));
    assert_eq!(output_accepted("cursor", &globs), Ok(()));
    assert!(matches!(
        output_accepted("cursor", LEGACY_UNINDENTED),
        Err(Reason::UnindentedContinuation { .. })
    ));
    assert_eq!(output_accepted("cursor", LIBRARY_SINGLE_QUOTED), Ok(()));
}
