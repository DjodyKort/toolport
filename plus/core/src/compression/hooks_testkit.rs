//! A scratch Claude settings file and data dir for the hook tests.

use super::hooks::{read_ledger, wrap};
use super::store::Paths;
use serde_json::{json, Value};
use std::path::PathBuf;
use toolport_plus_api::MemoryHost;

pub(super) struct World {
    _scratch: crate::randutil::ScratchDir,
    pub paths: Paths,
    pub host: MemoryHost,
    pub file: PathBuf,
}

pub(super) fn world(settings: Value) -> World {
    let scratch = crate::randutil::ScratchDir::new("hooks-kit");
    let dir = scratch.path().to_path_buf();
    let file = dir.join("settings.json");
    std::fs::write(&file, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    World {
        paths: Paths::new(&dir),
        host: MemoryHost {
            settings_paths: vec![file.clone()],
            ..Default::default()
        },
        file,
        _scratch: scratch,
    }
}

pub(super) fn rtk_settings() -> Value {
    json!({
        "theme": "dark",
        "hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "rtk hook claude"}]}
        ]}
    })
}

impl World {
    pub fn settings(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(&self.file).unwrap()).unwrap()
    }

    pub fn command(&self) -> String {
        self.settings()["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub fn set_command(&self, command: &str) {
        let mut settings = self.settings();
        settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = json!(command);
        std::fs::write(&self.file, settings.to_string()).unwrap();
    }

    pub fn health(&self, provider: &str) -> String {
        let status = super::hooks::status(&self.host, &self.paths).unwrap();
        status["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["provider"] == provider)
            .unwrap()["health"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub fn ledger_bytes(&self) -> Vec<u8> {
        std::fs::read(self.paths.dir.join(super::hooks::LEDGER_FILE)).unwrap_or_default()
    }

    pub fn wrapped_rtk(&self) -> String {
        wrap("rtk", "rtk hook claude")
    }

    pub fn entries(&self) -> usize {
        read_ledger(&self.paths).unwrap().entries.len()
    }
}
