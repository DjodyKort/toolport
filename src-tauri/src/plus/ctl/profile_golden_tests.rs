//! Replays the recorded mcpm reference outputs (`tests/fixtures/profile/mcpm`, produced by
//! `generator/gen.sh` against the Python reference in a synthetic config root) through
//! `toolportctl profile ...` and `client edit|import` on the equivalent synthetic registry. Every
//! difference from mcpm is a named rule in `expected_text`; an unlisted difference fails the test.

use super::output::table;
use super::profile_tests::{base_registry, cli, world_with, Cursor};
use serde_json::json;
use std::path::Path;

const FIXTURES: &str = "tests/fixtures/profile/mcpm";

#[derive(Clone, Copy)]
enum Rule {
    /// mcpm prints `\n` as literal text around some messages; the leading blank line goes too.
    LiteralNewlines,
    /// The ctl names the active profile under the table.
    ActiveProfile(&'static str),
    /// Hints name `toolportctl`, not `mcpm`.
    Brand,
    /// A client is cleaned of the gateway entry, not of `mcpm_profile_<name>`; the servers stay in
    /// the registry.
    GatewayEntry,
    /// mcpm prints a Python `\n` join inside the verbose cells; the ctl puts one server per line.
    VerboseTable,
    /// Failures go to stderr as `toolportctl: <message>`; mcpm's follow-up hints name mcpm commands.
    Error,
    /// Usage errors also point at `--help`.
    UsageError,
    /// `--no-clients` leaves the clients scoped to a profile that is gone; the ctl says which.
    LeftInPlace(&'static str),
    /// Only the first line is the same (the supported-client lists differ).
    FirstLine,
    /// mcpm reports the profile count; the ctl names the one profile the client follows.
    Follows(&'static str),
    /// The import preview stops at the interactive prompt in mcpm; the ctl lists the servers.
    ImportPreview,
    /// mcpm prints the selection and replacement dialogs; the ctl stops at the summary.
    ImportSummary,
    /// The ctl ends the import with the next step, where mcpm asks to replace the client config.
    ImportNext(&'static str),
    /// The ctl is honest about a flag that changes nothing; mcpm reports a creation.
    Replace(&'static str),
}

use Rule::*;

struct Case {
    name: &'static str,
    registry: Registry,
    cursor: Cursor,
    args: &'static [&'static str],
    code: i32,
    rules: &'static [Rule],
}

#[derive(Clone, Copy)]
enum Registry {
    Base,
    NoProfiles,
}

const fn case(
    name: &'static str,
    cursor: Cursor,
    args: &'static [&'static str],
    code: i32,
    rules: &'static [Rule],
) -> Case {
    Case {
        name,
        registry: Registry::Base,
        cursor,
        args,
        code,
        rules,
    }
}

use Cursor::{Bare, Direct, Missing, Scoped};

const CASES: &[Case] = &[
    case("profile-ls", Scoped, &["profile", "ls"], 0, &[LiteralNewlines, ActiveProfile("work")]),
    case("profile-ls-verbose", Scoped, &["profile", "ls", "--verbose"], 0, &[LiteralNewlines, VerboseTable, ActiveProfile("work")]),
    Case {
        registry: Registry::NoProfiles,
        ..case("profile-ls-empty", Bare, &["profile", "ls"], 0, &[LiteralNewlines])
    },
    case("profile-create", Scoped, &["profile", "create", "demo"], 0, &[LiteralNewlines, Brand]),
    case("profile-create-exists", Scoped, &["profile", "create", "work"], 1, &[Error]),
    case(
        "profile-create-force",
        Scoped,
        &["profile", "create", "work", "--force"],
        0,
        &[Replace("Profile 'work' already exists; nothing changed.")],
    ),
    case("profile-edit-name", Scoped, &["profile", "edit", "work", "--name", "job"], 0, &[LiteralNewlines]),
    case("profile-edit-servers", Scoped, &["profile", "edit", "work", "--servers", "beta,gamma"], 0, &[LiteralNewlines]),
    case("profile-edit-set-servers", Scoped, &["profile", "edit", "work", "--set-servers", "gamma"], 0, &[LiteralNewlines]),
    case("profile-edit-add", Scoped, &["profile", "edit", "work", "--add-server", "gamma"], 0, &[LiteralNewlines]),
    case("profile-edit-remove", Scoped, &["profile", "edit", "work", "--remove-server", "alpha"], 0, &[LiteralNewlines]),
    case(
        "profile-edit-name-servers",
        Scoped,
        &["profile", "edit", "play", "--name", "fun", "--add-server", "alpha"],
        0,
        &[LiteralNewlines],
    ),
    case("profile-edit-remove-absent", Scoped, &["profile", "edit", "work", "--remove-server", "gamma"], 0, &[LiteralNewlines]),
    case("profile-edit-unknown-server", Scoped, &["profile", "edit", "work", "--add-server", "ghost"], 1, &[Error]),
    case(
        "profile-edit-multiple",
        Scoped,
        &["profile", "edit", "work", "--servers", "alpha", "--add-server", "beta"],
        2,
        &[Error, UsageError],
    ),
    case("profile-edit-not-found", Scoped, &["profile", "edit", "ghost", "--name", "x"], 1, &[Error]),
    case("profile-edit-no-change", Scoped, &["profile", "edit", "work"], 0, &[LiteralNewlines]),
    case("profile-edit-same-set", Scoped, &["profile", "edit", "work", "--servers", "alpha,beta"], 0, &[LiteralNewlines]),
    case("profile-edit-name-taken", Scoped, &["profile", "edit", "work", "--name", "play"], 1, &[Error]),
    case("profile-rm", Scoped, &["profile", "rm", "work", "--force"], 0, &[GatewayEntry]),
    case("profile-rm-no-entry", Scoped, &["profile", "rm", "play", "--force"], 0, &[GatewayEntry]),
    case("profile-rm-empty", Scoped, &["profile", "rm", "empty", "--force"], 0, &[GatewayEntry]),
    case("profile-rm-no-clients", Scoped, &["profile", "rm", "work", "--force", "--no-clients"], 0, &[GatewayEntry, LeftInPlace("cursor")]),
    case("profile-rm-not-found", Scoped, &["profile", "rm", "ghost", "--force"], 1, &[Error]),
    case(
        "client-edit-set",
        Scoped,
        &["client", "edit", "cursor", "--set-profiles", "play"],
        0,
        &[Follows("profile 'play'")],
    ),
    case(
        "client-edit-remove",
        Scoped,
        &["client", "edit", "cursor", "--remove-profile", "work"],
        0,
        &[Follows("the active profile ('work')")],
    ),
    case("client-edit-clear", Scoped, &["client", "edit", "cursor", "--set-profiles", ""], 0, &[]),
    case(
        "client-edit-add-bare",
        Bare,
        &["client", "edit", "cursor", "--add-profile", "play"],
        0,
        &[Follows("profile 'play'")],
    ),
    case(
        "client-edit-set-bare",
        Bare,
        &["client", "edit", "cursor", "--set-profiles", "work"],
        0,
        &[Follows("profile 'work'")],
    ),
    case("client-edit-add-same", Scoped, &["client", "edit", "cursor", "--add-profile", "work"], 0, &[]),
    case("client-edit-none", Scoped, &["client", "edit", "cursor"], 0, &[]),
    case("client-edit-not-in-client", Scoped, &["client", "edit", "cursor", "--remove-profile", "play"], 0, &[]),
    case("client-edit-unknown-profile", Scoped, &["client", "edit", "cursor", "--add-profile", "ghost"], 1, &[Error]),
    case("client-edit-unknown-client", Scoped, &["client", "edit", "ghostclient", "--add-profile", "play"], 1, &[Error, FirstLine]),
    case(
        "client-edit-multiple",
        Scoped,
        &["client", "edit", "cursor", "--add-profile", "play", "--remove-profile", "work"],
        2,
        &[Error, UsageError],
    ),
    case(
        "client-add-two",
        Scoped,
        &["client", "edit", "cursor", "--add-profile", "play"],
        1,
        &[Replace(
            "toolportctl: Cursor already follows profile 'work'; a client follows one profile, so use --set-profiles to switch",
        )],
    ),
    case("client-import-preview", Direct, &["client", "import", "cursor"], 0, &[ImportPreview]),
    case(
        "client-import-select",
        Direct,
        &["client", "import", "cursor", "--select", "direct1,direct2"],
        0,
        &[ImportSummary, ImportNext("cursor")],
    ),
    case(
        "client-import-profile",
        Direct,
        &["client", "import", "cursor", "--select", "direct1,direct2", "--profile", "cursor"],
        0,
        &[ImportSummary, ImportNext("cursor")],
    ),
    case("client-import-no-config", Missing, &["client", "import", "cursor"], 1, &[Error]),
];

/// Cases whose mcpm run is not a plain command line (the interactive import is driven through a
/// patched prompt); their `args.txt` is not compared with the ctl arguments.
fn driven(name: &str) -> bool {
    matches!(
        name,
        "client-import-preview" | "client-import-select" | "client-import-profile"
    )
}

fn drop_leading_blank(text: &str) -> String {
    text.trim_start_matches('\n').to_string()
}

fn literal_newlines(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.starts_with(['┏', '┃', '┡', '│', '└']) {
                l.to_string()
            } else {
                l.replace("\\n", "\n")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn error_text(text: &str, usage: bool) -> String {
    let body = if text.lines().any(|l| l.starts_with("Error: ")) {
        text.lines()
            .skip_while(|l| !l.starts_with("Error: "))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.to_string()
    };
    let body = body.strip_prefix("Error: ").unwrap_or(&body);
    let body = body.split("\n\nAvailable options:").next().unwrap_or(body);
    let body = body
        .split("\nUse '--force' to overwrite")
        .next()
        .unwrap_or(body);
    let body = body.trim_end();
    if usage {
        format!("toolportctl: {body}\nRun `toolportctl --help` for usage.")
    } else {
        format!("toolportctl: {body}")
    }
}

fn rerender(text: &str) -> String {
    let (mut headers, mut rows): (Vec<String>, Vec<Vec<String>>) = (Vec::new(), Vec::new());
    let (mut before, mut after): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let is_table = line.starts_with(['┏', '┃', '┡', '│', '└']);
        if !is_table {
            if rows.is_empty() && headers.is_empty() {
                before.push(line);
            } else {
                after.push(line);
            }
        } else if let Some(cells) = line.strip_prefix('┃').or_else(|| line.strip_prefix('│')) {
            let cells: Vec<String> = cells
                .trim_end_matches(['┃', '│'])
                .split(['┃', '│'])
                .map(|c| c.trim().replace("\\n", "\n"))
                .collect();
            if line.starts_with('┃') {
                headers = cells;
            } else {
                rows.push(cells);
            }
        }
    }
    let heads: Vec<&str> = headers.iter().map(String::as_str).collect();
    let mut out: Vec<String> = before.iter().map(|l| l.to_string()).collect();
    out.push(table(&heads, &rows));
    out.extend(after.iter().map(|l| l.to_string()));
    out.join("\n")
}

fn expected_text(case: &Case, mcpm: &str, home: &str) -> String {
    let mut text = mcpm.replace("<HOME>", home);
    for rule in case.rules {
        text = match rule {
            LiteralNewlines => drop_leading_blank(&literal_newlines(&text)),
            ActiveProfile(name) => format!("{}\nActive profile: {name}", text.trim_end()),
            Brand => text.replace("'mcpm profile edit", "'toolportctl profile edit"),
            GatewayEntry => text
                .replace("remain available in global configuration", "remain in the registry")
                .replace("mcpm_profile_work", "gateway entry"),
            VerboseTable => rerender(&text),
            Error => error_text(&text, false),
            UsageError => format!("{text}\nRun `toolportctl --help` for usage."),
            LeftInPlace(client) => format!(
                "{}\nLeft in place: {client} (still scoped to the removed profile, they see no servers until re-scoped: toolportctl client edit <id> --set-profiles <profile>)",
                text.trim_end()
            ),
            FirstLine => text.lines().next().unwrap_or("").to_string(),
            Follows(who) => text
                .lines()
                .map(|l| match l.strip_prefix("✅ ") {
                    Some(rest) if rest.ends_with(" profiles and 0 servers configured") => {
                        format!("✅ Cursor follows {who}")
                    }
                    _ => l.to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            ImportPreview => text
                .replace("MCPM-managed servers:", "Toolport gateway entries:")
                .replace("Non-MCPM servers available for import:", "Direct servers available for import:")
                .replace("Non-MCPM servers:", "Direct servers:")
                .split("\nNo servers selected for import.")
                .next()
                .unwrap_or("")
                .to_string(),
            ImportSummary => import_summary(&text),
            ImportNext(client) => format!(
                "{}\n\nNext: toolportctl client sync --client {client}  (replaces the imported direct entries with the gateway entry)",
                text.trim_end()
            ),
            Replace(whole) => whole.to_string(),
        };
    }
    text
}

fn import_summary(text: &str) -> String {
    let text = text
        .replace("MCPM-managed servers:", "Toolport gateway entries:")
        .replace("Non-MCPM servers:", "Direct servers:")
        .replace("global configuration...", "the registry...")
        .replace("Non-MCPM servers available for import:\n\n", "")
        .replace("Created profile 'cursor'.\n", "");
    let mut kept = Vec::new();
    for line in text.lines() {
        if line.is_empty()
            && kept
                .last()
                .map_or(false, |l: &String| l.starts_with("Profile '"))
        {
            break;
        }
        if line.starts_with("This will replace") {
            break;
        }
        kept.push(line.to_string());
    }
    while kept.last().map_or(false, |l| l.is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

fn split_exit(output: &str) -> (&str, i32) {
    let (body, tail) = output.rsplit_once("[exit ").expect("an [exit N] line");
    (body, tail.trim().trim_end_matches(']').parse().unwrap())
}

fn expected_actual(case: &Case, expected_home: &str) -> (String, String) {
    let dir = Path::new(FIXTURES).join(case.name);
    let recorded = std::fs::read_to_string(dir.join("output.txt")).unwrap();
    let (body, _) = split_exit(&recorded);
    (
        expected_text(case, body.trim_end(), expected_home),
        recorded,
    )
}

fn registry_for(kind: Registry) -> serde_json::Value {
    match kind {
        Registry::Base => base_registry(),
        Registry::NoProfiles => {
            let mut empty = base_registry();
            empty["profiles"] = json!([]);
            empty
        }
    }
}

#[test]
fn every_recorded_case_is_replayed_and_every_replay_has_a_recording() {
    let mut recorded: Vec<String> = std::fs::read_dir(FIXTURES)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    recorded.sort();
    let mut known: Vec<&str> = CASES.iter().map(|c| c.name).collect();
    known.sort();
    assert_eq!(recorded, known);
}

#[test]
fn the_recorded_mcpm_arguments_match_the_replayed_ctl_arguments() {
    for case in CASES.iter().filter(|c| !driven(c.name)) {
        let args =
            std::fs::read_to_string(Path::new(FIXTURES).join(case.name).join("args.txt")).unwrap();
        assert_eq!(
            args.trim_end(),
            case.args.join(" ").trim_end(),
            "{}",
            case.name
        );
    }
}

#[test]
fn ctl_output_matches_the_recorded_mcpm_text_except_for_the_named_rules() {
    for case in CASES {
        world_with(registry_for(case.registry), case.cursor, |w| {
            let home = w.home.to_string_lossy().into_owned();
            let (expected, recorded) = expected_actual(case, &home);
            let expected = expected.trim_end().to_string();
            let (code, out, err) = cli(case.args);
            assert_eq!(code, case.code, "{}: exit code\n{out}{err}", case.name);
            let actual = if code == 0 { out } else { err };
            let actual = actual.trim_end();
            if matches!(case.name, "client-import-preview") {
                assert!(
                    actual.starts_with(&expected),
                    "{}\n--- expected prefix\n{expected}\n--- actual\n{actual}",
                    case.name
                );
            } else if case.rules.iter().any(|r| matches!(r, FirstLine)) {
                assert_eq!(
                    actual.lines().next().unwrap_or(""),
                    expected,
                    "{}",
                    case.name
                );
            } else {
                assert_eq!(
                    actual, expected,
                    "{}\n--- recorded mcpm output\n{recorded}",
                    case.name
                );
            }
        });
    }
}

#[test]
fn exit_codes_differ_from_mcpm_only_where_mcpm_reports_success_for_a_failure() {
    let mut differing: Vec<&str> = CASES
        .iter()
        .filter(|case| {
            let recorded =
                std::fs::read_to_string(Path::new(FIXTURES).join(case.name).join("output.txt"))
                    .unwrap();
            split_exit(&recorded).1 != case.code
        })
        .map(|case| case.name)
        .collect();
    differing.sort();
    assert_eq!(
        differing,
        [
            "client-add-two",
            "client-edit-multiple",
            "client-edit-unknown-client",
            "client-import-no-config",
            "profile-create-exists",
            "profile-edit-multiple",
            "profile-rm-not-found",
        ]
    );
}
