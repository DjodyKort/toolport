//! The keys of `context.json` the scan reads, and the one it writes (`sourceRoots`). The file is
//! plain JSON, so every other key stays as the file has it; a file that does not parse reads as
//! the defaults and is never overwritten.

use crate::context::bundle_io;
use serde_json::{Map, Value};
use std::path::Path;

pub const DEFAULT_CLIENTS_ROOT: &str = "~/Documents/GitHub/ExampleWorkspace/clients";
const SOURCE_ROOTS: &str = "sourceRoots";
const SOURCE_ROOTS_ALIAS: &str = "source_roots";
pub const ORG_PULL_KEY: &str = "orgPull";
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "fish",
    "ash",
    "pwsh",
    "cmd",
    "powershell",
];

/// How the presync refreshes the org file: the program and its arguments, run in the org tool's
/// folder. `command: None` keeps the built-in pull.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrgPull {
    pub command: Option<Vec<String>>,
    pub interval_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub clients_root: String,
    pub corp_tools_dir: Option<String>,
    pub cf_wrapper_hash: Option<String>,
    pub source_roots: Vec<String>,
    pub inert_patterns: Vec<String>,
    /// The `orgPull` key, or why it cannot be used.
    pub org_pull: Result<OrgPull, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            clients_root: DEFAULT_CLIENTS_ROOT.into(),
            corp_tools_dir: None,
            cf_wrapper_hash: None,
            source_roots: Vec::new(),
            inert_patterns: Vec::new(),
            org_pull: Ok(OrgPull::default()),
        }
    }
}

fn org_pull_error(why: &str) -> String {
    format!("{ORG_PULL_KEY} in context.json: {why}")
}

/// Reads the `orgPull` object. The command is a list because a string would need a shell to split
/// it; a shell as the program is refused for the same reason.
fn parse_org_pull(raw: Option<&Value>) -> Result<OrgPull, String> {
    let Some(raw) = raw else {
        return Ok(OrgPull::default());
    };
    let Value::Object(map) = raw else {
        return Err(org_pull_error(
            "must be an object like {\"command\": [\"program\", \"arg\"]}",
        ));
    };
    let command = match map.get("command") {
        None | Some(Value::Null) => None,
        Some(Value::String(_)) => {
            return Err(org_pull_error(
                "command must be a list of arguments, not a shell string",
            ))
        }
        Some(Value::Array(list)) => {
            let argv = list
                .iter()
                .map(|v| v.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| org_pull_error("command must contain only strings"))?;
            Some(check_argv(argv)?)
        }
        Some(_) => return Err(org_pull_error("command must be a list of arguments")),
    };
    let interval_secs = match map.get("intervalSecs") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64().filter(|n| *n > 0) {
            Some(n) => Some(n),
            None => {
                return Err(org_pull_error(
                    "intervalSecs must be a whole number of seconds above zero",
                ))
            }
        },
    };
    Ok(OrgPull {
        command,
        interval_secs,
    })
}

fn check_argv(argv: Vec<String>) -> Result<Vec<String>, String> {
    let Some(program) = argv.first() else {
        return Err(org_pull_error("command is empty; give the program to run"));
    };
    if program.trim().is_empty() {
        return Err(org_pull_error("the first item of command is empty"));
    }
    let name = Path::new(program)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    if SHELLS.contains(&name) {
        return Err(org_pull_error(
            "command must run the program itself, not a shell (no sh -c)",
        ));
    }
    if program.contains(char::is_whitespace) && !Path::new(program).is_absolute() {
        return Err(org_pull_error(
            "the first item of command holds a space; put the program alone first and each argument in its own item",
        ));
    }
    if argv.iter().any(|a| a.contains('\0')) {
        return Err(org_pull_error("command must not contain a NUL character"));
    }
    Ok(argv)
}

fn read_map(path: &Path) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(&bundle_io::read_exact(path).ok()??) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

fn strings(map: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| map.get(*key)?.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn load(path: &Path) -> Config {
    let Some(map) = read_map(path) else {
        return Config::default();
    };
    let text = |key: &str| map.get(key).and_then(Value::as_str).map(String::from);
    Config {
        clients_root: text("clients_root").unwrap_or_else(|| DEFAULT_CLIENTS_ROOT.into()),
        corp_tools_dir: text("corp_tools_dir"),
        cf_wrapper_hash: text("cf_wrapper_hash"),
        source_roots: strings(&map, &[SOURCE_ROOTS, SOURCE_ROOTS_ALIAS]),
        inert_patterns: strings(&map, &["inertPatterns", "inert_patterns"]),
        org_pull: parse_org_pull(map.get(ORG_PULL_KEY)),
    }
}

/// Whether [`set_source_roots`] can write `path`: it is missing or a JSON object.
pub fn check_writable(path: &Path) -> Result<(), String> {
    match bundle_io::read_exact(path)? {
        None => Ok(()),
        Some(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(_)) => Ok(()),
            _ => Err(format!(
                "{} is not a JSON object; fix or move it, then try again",
                path.display()
            )),
        },
    }
}

/// Replaces `sourceRoots` and leaves the other keys alone; a file that is not a JSON object is an
/// error and stays as it is.
pub fn set_source_roots(path: &Path, roots: &[String]) -> Result<(), String> {
    bundle_io::update(path, 3, |before| {
        let mut map = match before.map(serde_json::from_str::<Value>) {
            None => Map::new(),
            Some(Ok(Value::Object(map))) => map,
            Some(_) => {
                return Err(format!(
                    "{} is not a JSON object; fix or move it, then try again",
                    path.display()
                ))
            }
        };
        map.remove(SOURCE_ROOTS_ALIAS);
        if roots.is_empty() {
            map.remove(SOURCE_ROOTS);
        } else {
            map.insert(SOURCE_ROOTS.into(), roots.iter().cloned().collect());
        }
        let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
        Ok((bundle_io::Action::Write(text + "\n"), ()))
    })
}
