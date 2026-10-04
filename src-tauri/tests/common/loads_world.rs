#![allow(dead_code)]

//! A synthetic home shaped like a real client folder (MIG-SRC-2): a user memory file the
//! corporate clone provides, a workspace repository that holds a client repository, a deployed
//! client layer and personal rule, 33 library skills, one MCP server, plus the rows the first
//! `context loads` did not count (a plugin, commands, an agent, the memory index, an import
//! chain, files that load on demand). File sizes are chosen so the estimate (bytes / 4) of the
//! rows that existed before is exactly:
//!
//! user memory 10,070 · workspace memory 5,610 · client layer 83 · client rule 56 (path-scoped,
//! lazy) · personal rule 47 · skills 3,657 · MCP 40 · together 19,507 without the lazy row.
//!
//! Shared by the lib tests (`#[path]`) and the CLI golden tests. Nothing in it is real.

use std::path::{Path, PathBuf};

pub const USER_MEMORY_BYTES: usize = 40_280;
pub const WORKSPACE_MEMORY_BYTES: usize = 22_440;
pub const MANAGED_LOCAL_BYTES: usize = 332;
pub const CLIENT_RULE_BYTES: usize = 224;
pub const PERSONAL_RULE_BYTES: usize = 188;
pub const MCP_BYTES: usize = 160;
pub const SKILL_COUNT: usize = 33;
pub const BASELINE_TOKENS: u64 = 19_507;
pub const MEMORY_INDEX_LINES: usize = 300;

pub struct LoadsWorld {
    pub base: PathBuf,
    pub home: PathBuf,
    pub claude: PathBuf,
    pub workspace: PathBuf,
    pub client: PathBuf,
    pub corp: PathBuf,
    pub library: PathBuf,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn repo_marker(dir: &Path) {
    std::fs::create_dir_all(dir.join(".git")).unwrap();
}

/// `head` followed by filler lines, cut so the whole text is exactly `bytes` long.
fn sized(head: &str, bytes: usize) -> String {
    assert!(head.len() < bytes, "head of {} bytes does not fit {bytes}", head.len());
    let mut text = String::from(head);
    let mut n = 0;
    while text.len() < bytes {
        text.push_str(&format!("Rule {n}: keep the change small and say why.\n"));
        n += 1;
    }
    text.truncate(bytes - 1);
    text.push('\n');
    assert_eq!(text.len(), bytes);
    text
}

fn words(len: usize) -> String {
    let mut text: String = "helper notes for this area "
        .chars()
        .cycle()
        .take(len)
        .collect();
    if text.ends_with(' ') {
        text.pop();
        text.push('z');
    }
    text
}

fn skill_text(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\nBody of {name}.\n")
}

fn slug(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Builds the world into `base/home` without clearing `base`. `managed_header` is the first line
/// the context engine writes into a deployed `CLAUDE.local.md`.
pub fn build_in(base: &Path, managed_header: &str) -> LoadsWorld {
    let home = base.join("home");
    let claude = home.join(".claude");
    let w = LoadsWorld {
        base: base.to_path_buf(),
        claude: claude.clone(),
        workspace: home.join("work/erp"),
        client: home.join("work/erp/clients/acme-erp"),
        corp: home.join(".local/share/corp-tools"),
        library: home.join(".config/mcpm/skills_repo"),
        home: home.clone(),
    };

    put(
        &home.join(".config/mcpm/context.json"),
        "{\n  \"clients_root\": \"~/work/erp/clients\",\n  \"corp_tools_dir\": \"~/.local/share/corp-tools\"\n}\n",
    );

    // The corporate clone and what its sync deployed into ~/.claude.
    put(&w.corp.join("claude/CLAUDE.md"), "# Corp rules\nBe brief.\n");
    put(&w.corp.join("claude/commands/ship.md"), "# Ship the change\nRun the steps.\n");
    put(&claude.join("CLAUDE.md"), &sized("# Corp rules\n", USER_MEMORY_BYTES));
    put(&claude.join("commands/ship.md"), "# Ship the change\nRun the steps.\n");
    put(&claude.join("commands/local-note.md"), "# Write a local note\nKeep it short.\n");

    // The workspace repository: a memory file with an import chain, and a settings file that
    // belongs to a folder above the client repository.
    repo_marker(&w.workspace);
    put(
        &w.workspace.join("CLAUDE.md"),
        &sized(
            "# Workspace rules\nSee @docs/conventions.md for the conventions.\n",
            WORKSPACE_MEMORY_BYTES,
        ),
    );
    put(&w.workspace.join("docs/conventions.md"), &sized("# Conventions\n@naming.md\n", 400));
    put(&w.workspace.join("docs/naming.md"), &sized("# Naming\n@glossary.md\n", 300));
    put(&w.workspace.join("docs/glossary.md"), &sized("# Glossary\n", 200));
    put(
        &w.workspace.join(".claude/settings.local.json"),
        "{\n  \"enabledPlugins\": {\"kit@market\": false},\n  \"permissions\": {\"allow\": [\"Bash\"]}\n}\n",
    );

    // The client repository where Claude starts.
    repo_marker(&w.client);
    put(
        &w.client.join("CLAUDE.local.md"),
        &sized(&format!("{managed_header}\n\n"), MANAGED_LOCAL_BYTES),
    );
    put(&w.client.join(".claude/settings.json"), "{\n  \"permissions\": {\"allow\": [\"Read\"]}\n}\n");
    put(&w.client.join(".claude/commands/local-only.md"), "# Only in this client\nDo it.\n");
    put(&w.client.join("addons/billing/CLAUDE.md"), &sized("# Billing notes\n", 400));
    put(
        &w.client.join("addons/billing/.claude/skills/refund/SKILL.md"),
        &skill_text("refund", "Refund an order"),
    );
    put(&w.client.join("excluded-notes/CLAUDE.md"), &sized("# Notes nobody loads\n", 120));

    // Library layers and what was deployed from them.
    put(
        &w.library.join("rules/personal/SKILL.md"),
        "---\nname: personal\ndescription: Personal layer\nactivation: always\n---\nmine\n",
    );
    put(
        &w.library.join("rules/client-acme-erp/SKILL.md"),
        "---\nname: client-acme-erp\ndescription: Client layer\nglobs: \"clients/acme-erp/**\"\n---\nacme\n",
    );
    put(
        &claude.join("rules/personal.md"),
        &sized(
            "---\nname: personal\nactivation: always\n---\n\n",
            PERSONAL_RULE_BYTES,
        ),
    );
    put(
        &claude.join("rules/client-acme-erp.md"),
        &sized(
            "---\nname: client-acme-erp\nglobs: \"clients/acme-erp/**\"\n---\n\n",
            CLIENT_RULE_BYTES,
        ),
    );
    put(
        &w.library.join("agents/reviewer/AGENT.md"),
        "---\nname: reviewer\ndescription: Reviews a change\n---\nBody.\n",
    );
    put(
        &claude.join("agents/reviewer.md"),
        "---\nname: reviewer\ndescription: Reviews a change\n---\nBody.\n",
    );

    // 33 skills deployed to ~/.claude/skills: 27 of 444 bytes and 6 of 440 bytes as the
    // estimate counts them (name, ": ", description); the last three have no library copy.
    for n in 1..=SKILL_COUNT {
        let name = format!("skill-{n:02}");
        let total = if n <= 27 { 444 } else { 440 };
        let text = skill_text(&name, &words(total - name.len() - 2));
        put(&claude.join(format!("skills/{name}/SKILL.md")), &text);
        if n <= 30 {
            put(&w.library.join(format!("skills/{name}/SKILL.md")), &text);
        }
    }

    // One MCP server whose definition serialises to MCP_BYTES.
    let probe = serde_json::json!({"args": ["-y", ""], "command": "npx"});
    let pad = MCP_BYTES - serde_json::to_string(&probe).unwrap().len();
    let entry = serde_json::json!({"args": ["-y", "p".repeat(pad)], "command": "npx"});
    assert_eq!(serde_json::to_string(&entry).unwrap().len(), MCP_BYTES);
    put(
        &home.join(".claude.json"),
        &serde_json::to_string_pretty(&serde_json::json!({"mcpServers": {"docs-search": entry}})).unwrap(),
    );

    // A plugin that is on (two skills, an agent, two commands) and one that is off.
    let kit = claude.join("plugins/cache/market/kit/1.0.0");
    let idle = claude.join("plugins/cache/market/idle/0.3.0");
    put(&kit.join("skills/kit-build/SKILL.md"), &skill_text("kit-build", "Build the kit"));
    put(&kit.join("skills/kit-test/SKILL.md"), &skill_text("kit-test", "Test the kit"));
    put(
        &kit.join("agents/kit-agent.md"),
        "---\nname: kit-agent\ndescription: Works the kit\n---\nAgent.\n",
    );
    put(&kit.join("commands/kit-up.md"), "# Bring the kit up\nRun it.\n");
    put(&kit.join("commands/kit-down.md"), "# Take the kit down\nRun it.\n");
    put(&idle.join("skills/idle-skill/SKILL.md"), &skill_text("idle-skill", "Never on"));
    put(
        &claude.join("plugins/installed_plugins.json"),
        &format!(
            "{{\"version\":2,\"plugins\":{{\"kit@market\":[{{\"scope\":\"user\",\"installPath\":\"{}\",\"version\":\"1.0.0\"}}],\"idle@market\":[{{\"scope\":\"user\",\"installPath\":\"{}\",\"version\":\"0.3.0\"}}]}}}}",
            kit.display(),
            idle.display()
        ),
    );
    put(
        &claude.join("settings.json"),
        "{\n  \"enabledPlugins\": {\"kit@market\": true, \"idle@market\": false},\n  \"claudeMdExcludes\": [\"**/excluded-notes/CLAUDE.md\"],\n  \"permissions\": {\"allow\": [\"Read\"]}\n}\n",
    );

    // The auto-memory index of the client repository.
    let index: String = (1..=MEMORY_INDEX_LINES)
        .map(|n| format!("- note {n}: a fact worth remembering\n"))
        .collect();
    put(
        &claude.join(format!("projects/{}/memory/MEMORY.md", slug(&w.client))),
        &index,
    );
    w
}
