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
