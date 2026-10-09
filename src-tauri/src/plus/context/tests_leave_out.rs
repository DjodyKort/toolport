use super::leave_out::{self, Args, Op, Target, World, AUTO_MEMORY_OFF};
use super::loads::{what_loads_with, LoadsOptions, WhatLoads};
use super::roots::Roots;
use super::tests_loads::{config, Home};
use crate::plus::op::ErrorKind;
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Fx {
    h: Home,
    roots: Roots,
    data: PathBuf,
    cwd: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let h = Home::new();
        h.put(".claude/CLAUDE.md", "global rules\n");
        h.put("work/app/CLAUDE.md", "project rules\n");
        let cwd = h.repo("work/app");
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        let roots = h.roots();
        let data = h.at("data");
        std::fs::create_dir_all(&data).unwrap();
        Self { h, roots, data, cwd }
    }

    fn run(&self, op: Op, target: Target, dry_run: bool) -> Result<Value, crate::plus::op::OpError> {
        let config = config(json!({}));
        leave_out::run(
            &World { roots: &self.roots, config: &config, data_dir: &self.data },
            &Args { op, target, cwd: &self.cwd, dry_run },
        )
    }

    fn file(&self, rel: &str) -> Target {
        Target::File(self.h.at(rel).display().to_string())
    }

    fn stack(&self) -> WhatLoads {
        let config = config(json!({}));
        let mut report =
            what_loads_with(&self.roots, &config, None, &self.cwd, &LoadsOptions::default()).unwrap();
        leave_out::mark(&mut report.items, &self.data, &self.cwd);
        report
    }

    fn settings(&self) -> PathBuf {
        self.cwd.join(".claude/settings.local.json")
    }
}

fn row<'a>(report: &'a WhatLoads, name_end: &str) -> &'a super::loads::LoadItem {
    report
        .items
        .iter()
        .find(|i| i.path.as_deref().is_some_and(|p| p.ends_with(name_end)))
        .unwrap_or_else(|| panic!("no row for {name_end}"))
}

fn snapshot(path: &Path) -> String {
    format!("{:?}", tree_snapshot(path))
}

#[test]
fn a_left_out_file_leaves_the_stack_and_loading_it_again_restores_the_folder() {
    let fx = Fx::new();
    let before = snapshot(&fx.cwd);
    let tokens = fx.stack().total_tokens;
    let global = row(&fx.stack(), ".claude/CLAUDE.md").tokens;

    let preview = fx.run(Op::Add, fx.file(".claude/CLAUDE.md"), true).unwrap();
    assert_eq!(preview["dryRun"], true);
    assert_eq!(snapshot(&fx.cwd), before);
    assert_eq!(preview["plan"]["effects"]["tokens"]["before"], tokens);
    assert_eq!(preview["plan"]["effects"]["tokens"]["after"], tokens - global);

    let done = fx.run(Op::Add, fx.file(".claude/CLAUDE.md"), false).unwrap();
    assert_eq!(done["leftOut"].as_array().unwrap().len(), 1);
    let stack = fx.stack();
    let item = row(&stack, ".claude/CLAUDE.md");
    assert!(!item.loaded);
    assert_eq!(item.reason, "excluded by claudeMdExcludes");
    assert_eq!(item.left_out_by, Some("toolport"));
    assert_eq!(stack.total_tokens, tokens - global);
    assert!(row(&stack, "work/app/CLAUDE.md").loaded);

    let again = fx.run(Op::Rm, fx.file(".claude/CLAUDE.md"), false).unwrap();
    assert_eq!(again["leftOut"].as_array().unwrap().len(), 0);
    assert_eq!(snapshot(&fx.cwd), before);
    assert_eq!(fx.stack().total_tokens, tokens);
}

#[test]
fn several_files_and_the_memory_switch_are_owned_together_and_undone_one_by_one() {
    let fx = Fx::new();
    fx.run(Op::Add, fx.file(".claude/CLAUDE.md"), false).unwrap();
    fx.run(Op::Add, fx.file("work/app/CLAUDE.md"), false).unwrap();
    fx.run(Op::Add, Target::Memory, false).unwrap();
    let text = std::fs::read_to_string(fx.settings()).unwrap();
    assert!(text.contains("autoMemoryEnabled") && text.matches("CLAUDE.md").count() == 2);
    let (files, memory) = leave_out::owned(&fx.data, &fx.cwd);
    assert_eq!(files.len(), 2);
    assert!(memory);

    fx.run(Op::Rm, fx.file(".claude/CLAUDE.md"), false).unwrap();
    let (files, memory) = leave_out::owned(&fx.data, &fx.cwd);
    assert_eq!(files, vec![fx.h.at("work/app/CLAUDE.md").display().to_string()]);
    assert!(memory);

    fx.run(Op::Rm, Target::Memory, false).unwrap();
    fx.run(Op::Rm, fx.file("work/app/CLAUDE.md"), false).unwrap();
    assert!(!fx.settings().exists(), "the file Toolport created goes with its last key");
}

#[test]
fn auto_memory_off_removes_the_memory_row_and_is_attributed_to_toolport() {
    let fx = Fx::new();
    let slug: String = fx
        .cwd
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    fx.h.put(&format!(".claude/projects/{slug}/memory/MEMORY.md"), "- a note\n");
    assert!(row_of_kind(&fx.stack(), "memory-index").loaded);

    fx.run(Op::Add, Target::Memory, false).unwrap();
    let stack = fx.stack();
    let memory = row_of_kind(&stack, "memory-index");
    assert!(!memory.loaded);
    assert_eq!(memory.reason, AUTO_MEMORY_OFF);
    assert_eq!(memory.left_out_by, Some("toolport"));
}

fn row_of_kind<'a>(report: &'a WhatLoads, kind: &str) -> &'a super::loads::LoadItem {
    report.items.iter().find(|i| i.kind == kind).unwrap()
}

#[test]
fn a_file_left_out_by_other_settings_is_not_taken_over() {
    let fx = Fx::new();
    let global = fx.h.at(".claude/CLAUDE.md").display().to_string();
    let mine = format!("{{\n  \"claudeMdExcludes\": [\"{global}\"]\n}}\n");
    fx.h.put("work/app/.claude/settings.local.json", &mine);

    let stack = fx.stack();
    assert_eq!(row(&stack, ".claude/CLAUDE.md").left_out_by, Some("settings"));

    let done = fx.run(Op::Add, fx.file(".claude/CLAUDE.md"), false).unwrap();
    let warnings = done["plan"]["warnings"].to_string();
    assert!(warnings.contains("already left out by settings Toolport did not write"), "{warnings}");
    assert_eq!(std::fs::read_to_string(fx.settings()).unwrap(), mine);
    fx.run(Op::Rm, fx.file(".claude/CLAUDE.md"), false).unwrap();
    assert_eq!(std::fs::read_to_string(fx.settings()).unwrap(), mine);
}

#[test]
fn a_managed_policy_file_a_stranger_and_a_relative_path_are_refused() {
    let mut fx = Fx::new();
    let managed = fx.h.put("managed/CLAUDE.md", "policy\n");
    fx.roots.managed_claude_md = Some(managed.clone());
    let refused = fx
        .run(Op::Add, Target::File(managed.display().to_string()), false)
        .unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Conflict);
    assert!(refused.message.contains("managed policy"));

    fx.h.put("work/app/README.md", "not an instruction file\n");
    let stranger = fx.run(Op::Add, fx.file("work/app/README.md"), false).unwrap_err();
    assert_eq!(stranger.kind, ErrorKind::NotFound);

    let relative = fx.run(Op::Add, Target::File("CLAUDE.md".into()), false).unwrap_err();
    assert_eq!(relative.kind, ErrorKind::Usage);
    assert!(!fx.settings().exists());
}

#[test]
fn a_path_written_with_a_symlinked_prefix_is_stored_as_claude_code_sees_it() {
    let fx = Fx::new();
    let link = fx.h.at("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fx.h.at("work"), &link).unwrap();
    let through_link = Target::File(link.join("app/CLAUDE.md").display().to_string());
    let done = fx.run(Op::Add, through_link, false).unwrap();
    assert_eq!(
        done["leftOut"][0].as_str().unwrap(),
        fx.h.at("work/app/CLAUDE.md").display().to_string()
    );
    let again = fx
        .run(Op::Rm, Target::File(link.join("app/CLAUDE.md").display().to_string()), false)
        .unwrap();
    assert_eq!(again["leftOut"].as_array().unwrap().len(), 0);
}
