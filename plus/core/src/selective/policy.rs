//! What can keep a ticked item from loading although the plan applied cleanly: Claude Code's
//! `disableAllHooks` setting and an administrator's marketplace restrictions.

use super::ops::Env;
use crate::plugins::link::Settings;
use crate::plugins::settings::{Layers, Scope};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

const HOOKS_KEY: &str = "disableAllHooks";
const ALLOWED_KEY: &str = "strictKnownMarketplaces";
const BLOCKED_KEY: &str = "blockedMarketplaces";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Policy {
    pub hooks_off: Option<&'static str>,
    pub blocked: Option<&'static str>,
}

impl Policy {
    pub fn to_json(&self) -> Value {
        json!({"hooksOff": self.hooks_off, "blocked": self.blocked})
    }
}

fn names(entry: &Value, root: &str) -> bool {
    match entry["source"].as_str() {
        Some("directory" | "file") => entry["path"].as_str() == Some(root),
        Some("pathPattern") => entry["pathPattern"]
            .as_str()
            .and_then(|pattern| Regex::new(pattern).ok())
            .is_some_and(|re| re.is_match(root)),
        _ => false,
    }
}

fn listed(layers: &Layers, key: &str, root: &str) -> Option<bool> {
    let entries = layers.managed(key)?.as_array()?;
    Some(entries.iter().any(|e| names(e, root)))
}

/// `market_root` is the folder of the generated marketplace, as the policy sees it.
pub fn read(env: &Env, market_root: &Path) -> Policy {
    let layers = Layers::load(&env.plugins.claude_home, None, env.managed.as_deref(), None);
    let layered = layers.flag(HOOKS_KEY);
    let user =
        Settings::load(env.store, env.plugins).is_ok_and(|s| s.doc[HOOKS_KEY] == json!(true));
    let hooks_off = match layered {
        Some((on, Scope::Managed)) => on.then_some("the managed policy"),
        _ => (user || layered.is_some_and(|(on, _)| on)).then_some("your Claude Code settings"),
    };
    let root = market_root.to_string_lossy();
    let blocked = (listed(&layers, ALLOWED_KEY, &root) == Some(false)
        || listed(&layers, BLOCKED_KEY, &root) == Some(true))
    .then_some("the managed policy");
    Policy { hooks_off, blocked }
}
