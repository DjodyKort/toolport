use serde_json::{json, Value};
use tempfile::TempDir;
use toolport_plus_api::MemoryHost;

use super::call_tool;

const SKILL: &str = "---\nname: nightly\ndescription: Runs the nightly checks\n---\n\n# Nightly\n\nRun the checks.\n";

fn host() -> MemoryHost {
    MemoryHost {
        registry: toolport_plus_api::MemoryRegistry::with_servers(Vec::new()),
        seam: true,
        ..Default::default()
    }
}

fn library() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("skills/nightly")).unwrap();
    std::fs::write(dir.path().join("skills/nightly/SKILL.md"), SKILL).unwrap();
    let repo = dir.path().to_string_lossy().into_owned();
    (dir, repo)
}

fn call(host: &MemoryHost, tool: &str, args: Value) -> Value {
    call_tool(host, tool, &args).unwrap()
}

#[test]
fn test01_the_body_a_get_returns_written_back_leaves_the_file_as_it_was() {
    let host = host();
    let (dir, repo) = library();
    let file = dir.path().join("skills/nightly/SKILL.md");
    let first = call(
        &host,
        "skills_get",
        json!({"name": "nightly", "repo_path": repo}),
    );
    let before = first["body"].as_str().unwrap().to_string();
    call(
        &host,
        "skills_edit_body",
        json!({"name": "nightly", "new_body": "Something else.", "confirm": true, "repo_path": repo}),
    );
    assert_ne!(std::fs::read_to_string(&file).unwrap(), SKILL);
    call(
        &host,
        "skills_edit_body",
        json!({"name": "nightly", "new_body": before, "confirm": true, "repo_path": repo}),
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), SKILL);
}

#[test]
fn test02_trailing_blank_lines_of_a_body_are_not_kept_so_the_diff_ignores_them() {
    let host = host();
    let (dir, repo) = library();
    call(
        &host,
        "skills_edit_body",
        json!({"name": "nightly", "new_body": "Run the checks.\n\n\n", "confirm": true, "repo_path": repo}),
    );
    let text = std::fs::read_to_string(dir.path().join("skills/nightly/SKILL.md")).unwrap();
    assert!(text.ends_with("---\n\nRun the checks.\n"), "{text:?}");
}

#[test]
fn test03_an_edit_keeps_the_blank_line_after_the_closing_fence() {
    let host = host();
    let (dir, repo) = library();
    call(
        &host,
        "skills_edit_body",
        json!({"name": "nightly", "new_body": "New text.", "confirm": true, "repo_path": repo}),
    );
    let text = std::fs::read_to_string(dir.path().join("skills/nightly/SKILL.md")).unwrap();
    assert_eq!(
        text,
        "---\nname: nightly\ndescription: Runs the nightly checks\n---\n\nNew text.\n"
    );
}

const COMMENTED: &str = "---\nname: nightly\n# what it does\ndescription: Runs the nightly checks\nallowed-tools: Bash(git *)\n---\n\n# Nightly\n\nRun the checks.\n";

fn commented_library() -> (TempDir, String) {
    let (dir, repo) = library();
    std::fs::write(dir.path().join("skills/nightly/SKILL.md"), COMMENTED).unwrap();
    (dir, repo)
}

fn edit(
    host: &MemoryHost,
    repo: &str,
    patch: Value,
    extra: Value,
) -> Result<Value, super::ToolError> {
    let mut args = json!({"name": "nightly", "patch": patch, "repo_path": repo});
    args.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    call_tool(host, "skills_edit_frontmatter", &args)
}

#[test]
fn test04_a_field_edit_leaves_comments_key_spelling_and_the_body_alone() {
    let host = host();
    let (dir, repo) = commented_library();
    let out = edit(
        &host,
        &repo,
        json!({"description": "Runs the checks at night", "allowed_tools": "Read"}),
        json!({"confirm": true}),
    )
    .unwrap();
    assert_eq!(out["changed"], json!(["allowed_tools", "description"]));
    let text = std::fs::read_to_string(dir.path().join("skills/nightly/SKILL.md")).unwrap();
    assert_eq!(
        text,
        "---\nname: nightly\n# what it does\ndescription: Runs the checks at night\nallowed-tools: Read\n---\n\n# Nightly\n\nRun the checks.\n"
    );
}

#[test]
fn test05_a_dry_run_writes_nothing_and_returns_the_fields_the_new_text_and_the_lint() {
    let host = host();
    let (dir, repo) = commented_library();
    let long = "x".repeat(1100);
    let out = edit(
        &host,
        &repo,
        json!({"description": long}),
        json!({"dry_run": true}),
    )
    .unwrap();
    assert_eq!(out["dryRun"], true);
    assert!(out.get("newHash").is_none());
    assert_eq!(out["fields"]["allowed-tools"], "Bash(git *)");
    assert!(out["after"].as_str().unwrap().contains("xxxx"));
    let lint = out["lint"].as_array().unwrap();
    assert!(
        lint.iter()
            .any(|row| row["level"] == "warning"
                && row["message"].as_str().unwrap().contains("1100")),
        "{lint:?}"
    );
    let text = std::fs::read_to_string(dir.path().join("skills/nightly/SKILL.md")).unwrap();
    assert_eq!(text, COMMENTED);
}

#[test]
fn test06_an_empty_dry_run_patch_reads_the_fields_without_a_confirm() {
    let host = host();
    let (_dir, repo) = commented_library();
    let out = edit(&host, &repo, json!({}), json!({"dry_run": true})).unwrap();
    assert_eq!(out["fields"]["description"], "Runs the nightly checks");
    assert_eq!(out["changed"], json!([]));
}

#[test]
fn test07_a_change_that_fails_the_skill_check_is_refused_with_the_reason_and_writes_nothing() {
    let host = host();
    let (dir, repo) = commented_library();
    let over = "x".repeat(1537);
    let err = edit(
        &host,
        &repo,
        json!({"description": over}),
        json!({"confirm": true}),
    )
    .unwrap_err();
    assert_eq!(err.kind, "invalid_input");
    assert!(
        err.message.contains("description must be 1-1536"),
        "{}",
        err.message
    );
    let text = std::fs::read_to_string(dir.path().join("skills/nightly/SKILL.md")).unwrap();
    assert_eq!(text, COMMENTED);
}

#[test]
fn test08_when_to_use_counts_with_the_description_and_null_removes_a_field() {
    let host = host();
    let (dir, repo) = commented_library();
    let err = edit(
        &host,
        &repo,
        json!({"when_to_use": "u".repeat(1530)}),
        json!({"dry_run": true}),
    )
    .unwrap_err();
    assert!(
        err.message.contains("when_to_use (1530)"),
        "{}",
        err.message
    );
    edit(
        &host,
        &repo,
        json!({"allowed-tools": Value::Null}),
        json!({"confirm": true}),
    )
    .unwrap();
    let text = std::fs::read_to_string(dir.path().join("skills/nightly/SKILL.md")).unwrap();
    assert!(!text.contains("allowed"));
}

#[test]
fn test09_a_write_needs_a_confirm_and_a_preview_does_not() {
    let host = host();
    let (_dir, repo) = commented_library();
    let refused = edit(&host, &repo, json!({"license": "MIT"}), json!({}));
    assert_eq!(refused.unwrap_err().kind, "refused");
    assert!(edit(
        &host,
        &repo,
        json!({"license": "MIT"}),
        json!({"dry_run": true})
    )
    .is_ok());
}
