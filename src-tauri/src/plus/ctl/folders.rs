//! `toolportctl context folders`: active gateway profile per folder and the folder-profiles switch.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::plus::dispatch;
use serde_json::{json, Value};

const FOLDERS: Spec = Spec {
    flags: &[switch("--enable"), switch("--disable"), value("--cwd")],
    inline: Inline::Value,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub fn folders(rest: &[String]) -> Result<Output, CtlError> {
    let flags = FOLDERS.parse(rest)?;
    let (enable, disable) = (flags.on("--enable"), flags.on("--disable"));
    if enable && disable {
        return Err(CtlError::usage("--enable and --disable are exclusive"));
    }
    if enable || disable {
        dispatch("plus.context.folderProfilesSet", json!({"enabled": enable}))
            .map_err(|e| CtlError::failed("folders_set", e))?;
    }
    let cwd = match flags.one("--cwd") {
        Some(c) => c.to_string(),
        None => std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    };
    let data = dispatch("plus.context.folderProfiles", json!({"cwd": cwd}))
        .map_err(|e| CtlError::failed("folders", e))?;
    let enabled = data["enabled"].as_bool().unwrap_or(false);
    let mut human = format!(
        "folder profiles: {}\n",
        if enabled { "enabled" } else { "disabled" }
    );
    for f in data["folders"].as_array().into_iter().flatten() {
        let name = |k: &str| f[k].as_str().map(String::from);
        let active = name("profile")
            .or_else(|| name("wouldApply").map(|p| format!("{p} (inactive)")))
            .unwrap_or_else(|| "default".into());
        human.push_str(&format!(
            "{}  ->  {}  ~{} tokens\n  {}\n",
            f["root"].as_str().unwrap_or(""),
            active,
            f["tokens"],
            f["reason"].as_str().unwrap_or("")
        ));
    }
    let mappings = data["mappings"].as_array().map_or(0, Vec::len);
    human.push_str(&format!("{mappings} mapping(s) configured\n"));
    Ok(Output::new(Value::clone(&data), human))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{self, FolderProfile, Registry};

    #[test]
    fn enable_disable_flags_toggle_and_report() {
        let _lock = registry::data_dir_test_lock();
        let dir = std::env::temp_dir().join(format!("ctl-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = registry::DataDirOverride::set(&dir);
        let mut reg = Registry::default();
        let id = reg.add_profile("Work");
        reg.folder_profiles = vec![FolderProfile {
            path: "/proj/work".into(),
            profile: id,
        }];
        registry::save(&reg).unwrap();
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let off = folders(&args(&["--cwd", "/proj/work/app"])).unwrap();
        assert_eq!(off.data["enabled"], false);
        assert!(off.human.contains("Work (inactive)"));
        let on = folders(&args(&["--enable", "--cwd=/proj/work/app"])).unwrap();
        assert_eq!(on.data["enabled"], true);
        assert!(on.human.contains("->  Work  "));
        let off = folders(&args(&["--disable", "--cwd", "/proj/work/app"])).unwrap();
        assert_eq!(off.data["enabled"], false);
        assert!(folders(&args(&["--enable", "--disable"])).is_err());
    }
}
