//! `toolportctl usage`: token and MCP usage from the Claude Code transcript index.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::dispatch;
use serde_json::{json, Value};

const USAGE: &str = "usage: usage [--root <projects dir>] [--no-refresh]";

const SPEC: Spec = Spec {
    flags: &[
        switch("--no-refresh"),
        value("--root").needs("a directory").nonempty(),
    ],
    inline: Inline::Value,
    unknown: Unknown::NamedUsage(USAGE),
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SPEC.parse(rest)?;
    let mut args = json!({"refresh": !flags.on("--no-refresh")});
    if let Some(root) = flags.one("--root") {
        args["root"] = Value::String(root.to_string());
    }
    let data = dispatch("plus.obs.summary", args).map_err(CtlError::usage)?;
    let human = render(&data);
    Ok(Output::new(data, human))
}

fn count(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}

fn tokens_line(v: &Value) -> String {
    format!(
        "{:>6} msgs  in {:>9}  out {:>9}  cache-write {:>10}  cache-read {:>11}",
        count(v, "messages"),
        count(v, "input"),
        count(v, "output"),
        count(v, "cacheCreation"),
        count(v, "cacheRead")
    )
}

fn section(human: &mut String, title: &str, rows: &Value) {
    let Some(map) = rows.as_object().filter(|m| !m.is_empty()) else {
        return;
    };
    human.push_str(&format!("{title}\n"));
    for (name, row) in map {
        human.push_str(&format!("  {name:<28} {}\n", tokens_line(row)));
    }
}

fn render(data: &Value) -> String {
    let mut human = format!(
        "indexed {} messages from {} transcript files\ntotal\n  {}\n",
        count(&data["index"], "messages"),
        count(&data["index"], "files"),
        tokens_line(&data["totals"]),
    );
    section(&mut human, "by day", &data["byDay"]);
    section(&mut human, "by model", &data["byModel"]);
    if let Some(map) = data["byMcpServer"].as_object().filter(|m| !m.is_empty()) {
        human.push_str("mcp servers\n");
        for (server, agg) in map {
            human.push_str(&format!("  {server:<28} {} calls\n", count(agg, "calls")));
        }
    }
    let failures = data["mcpFailures"].as_array().map_or(0, Vec::len);
    if failures > 0 {
        human.push_str(&format!("mcp connection failures: {failures}\n"));
    }
    human
}

#[cfg(test)]
mod tests {
    use crate::plus::ctl::run_with;
    use crate::plus::obs::transcript::fixtures::assistant;
    use crate::plus::testutil::DataDirFx;
    use serde_json::Value;
    use std::path::Path;

    fn argv(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn run(list: &[&str]) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with(&argv(list), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn write_transcripts(root: &Path) {
        let project = root.join("demo-project");
        std::fs::create_dir_all(&project).unwrap();
        let lines = [
            assistant(
                "m1",
                "s1",
                "2026-10-01T10:00:00Z",
                "model-a",
                (10, 40, 500, 7000),
                &[("t1", "mcp__alpha__list")],
            ),
            assistant(
                "m2",
                "s1",
                "2026-10-02T09:00:00Z",
                "model-b",
                (3, 4, 0, 100),
                &[("t2", "mcp__alpha__get"), ("t3", "Bash")],
            ),
        ];
        std::fs::write(project.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn usage_json_aggregates_a_synthetic_transcript_root() {
        let fx = DataDirFx::new("toolportctl-usage", "json");
        let root = fx.dir.join("projects");
        write_transcripts(&root);
        let root = root.to_string_lossy().into_owned();
        let (code, out, err) = run(&["--json", "usage", "--root", &root]);
        assert_eq!(code, 0, "{err}");
        let value: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(value["command"], "usage");
        let data = &value["data"];
        assert_eq!(data["totals"]["messages"], 2);
        assert_eq!(data["totals"]["input"], 13);
        assert_eq!(data["totals"]["cacheRead"], 7100);
        assert_eq!(data["byDay"]["2026-10-01"]["output"], 40);
        assert_eq!(data["byModel"]["model-b"]["messages"], 1);
        assert_eq!(data["byMcpServer"]["alpha"]["calls"], 2);
        assert_eq!(data["index"]["files"], 1);
    }

    #[test]
    fn usage_human_output_lists_days_models_and_servers() {
        let fx = DataDirFx::new("toolportctl-usage", "human");
        let root = fx.dir.join("projects");
        write_transcripts(&root);
        let root = format!("--root={}", root.display());
        let (code, out, err) = run(&["usage", &root]);
        assert_eq!(code, 0, "{err}");
        for needle in [
            "indexed 2 messages from 1 transcript files",
            "by day",
            "2026-10-02",
            "by model",
            "model-a",
            "mcp servers",
            "alpha",
            "2 calls",
        ] {
            assert!(out.contains(needle), "{needle:?} missing in:\n{out}");
        }
    }

    #[test]
    fn no_refresh_reads_only_the_existing_index() {
        let fx = DataDirFx::new("toolportctl-usage", "norefresh");
        let root = fx.dir.join("projects");
        write_transcripts(&root);
        let root = root.to_string_lossy().into_owned();
        let (code, out, _) = run(&["--json", "usage", "--no-refresh", "--root", &root]);
        assert_eq!(code, 0);
        let value: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(value["data"]["index"]["messages"], 0);
        let (code, out, _) = run(&["--json", "usage", "--root", &root]);
        assert_eq!(code, 0);
        let value: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(value["data"]["index"]["messages"], 2);
        let (code, out, _) = run(&["--json", "usage", "--no-refresh", "--root", &root]);
        assert_eq!(code, 0);
        let value: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(value["data"]["index"]["messages"], 2);
    }

    #[test]
    fn usage_rejects_unknown_and_incomplete_arguments() {
        let _fx = DataDirFx::new("toolportctl-usage", "usage");
        let (code, _, err) = run(&["usage", "--bogus"]);
        assert_eq!(code, 2, "{err}");
        let (code, _, _) = run(&["usage", "--root"]);
        assert_eq!(code, 2);
        let (code, _, _) = run(&["usage", "--root="]);
        assert_eq!(code, 2);
        let (code, _, _) = run(&["usage", "stray"]);
        assert_eq!(code, 2);
    }
}
