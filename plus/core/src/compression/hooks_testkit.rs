//! A scratch Claude settings file and data dir for the hook tests.

use super::store::Paths;
use serde_json::{json, Value};
use toolport_plus_api::MemoryHost;

pub(super) struct World {
    _scratch: crate::randutil::ScratchDir,
    pub paths: Paths,
    pub host: MemoryHost,
}

pub(super) fn world(settings: Value) -> World {
    let scratch = crate::randutil::ScratchDir::new("hooks-kit");
    let dir = scratch.path().to_path_buf();
    let file = dir.join("settings.json");
    std::fs::write(&file, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
    World {
        paths: Paths::new(&dir),
        host: MemoryHost {
            settings_paths: vec![file],
            ..Default::default()
        },
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
    pub fn ledger_bytes(&self) -> Vec<u8> {
        std::fs::read(self.paths.dir.join(super::hooks::LEDGER_FILE)).unwrap_or_default()
    }
}
