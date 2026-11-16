//! Registration of the self-management server (`toolport-plus-self`) and where it is switched on.
//!
//! The server is one registry entry with a global switch. Which clients see it follows upstream's
//! access sets: it is listed in the default access set when that is an explicit set, and in every
//! set a client is scoped to, once per set (`plus.selfmcp.enabledIn`), so taking it out of a set
//! later sticks. With no explicit default set the default is every enabled server and needs no
//! listing.

use std::path::PathBuf;

use toolport_plus_api::{Host, RegistryTx, RegistryView, SelfMcpRecord, ServerRecord};

use super::{BINARY_NAME, SERVER_NAME};

pub const SELF_SOURCE: &str = "plus:selfmcp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Created,
    Updated,
    Unchanged,
    OptedOut,
}

impl Ensured {
    pub fn as_str(self) -> &'static str {
        match self {
            Ensured::Created => "created",
            Ensured::Updated => "updated",
            Ensured::Unchanged => "unchanged",
            Ensured::OptedOut => "opted-out",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub outcome: Ensured,
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Missing,
    OptedOut,
    NotEnabled,
    Enabled,
    Disabled,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Missing => "missing",
            Standing::OptedOut => "opted-out",
            Standing::NotEnabled => "not-enabled",
            Standing::Enabled => "enabled",
            Standing::Disabled => "disabled",
        }
    }
}

/// The server's standing in one access set: listed in it, or taken out on purpose after
/// Toolport had listed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetState {
    pub id: String,
    pub member: bool,
    pub opted_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub standing: Standing,
    /// The global switch.
    pub switched_on: bool,
    /// The default access set, when it is an explicit one.
    pub default_access: Option<SetState>,
    pub client_access_sets: Vec<SetState>,
}

fn binary_file() -> String {
    format!("{BINARY_NAME}{}", std::env::consts::EXE_SUFFIX)
}

/// Absolute path of `toolport-selfmcp` next to the running binary (the gateway ships in the same
/// directory), else in the data directory's `bin`, else the bare name for PATH lookup.
pub fn binary_path(host: &dyn Host) -> String {
    let file = binary_file();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.parent().map(PathBuf::from));
    }
    if let Some(dir) = host.data_dir() {
        dirs.push(dir.join("bin"));
    }
    for dir in &dirs {
        let candidate = dir.join(&file);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    match dirs.first() {
        Some(dir) => dir.join(&file).to_string_lossy().into_owned(),
        None => file,
    }
}

fn is_self(server: &ServerRecord) -> bool {
    server.text("source") == Some(SELF_SOURCE)
}

pub fn find_self(servers: &[ServerRecord]) -> Option<&ServerRecord> {
    servers.iter().find(|s| is_self(s))
}

fn enabled_in(record: &Option<SelfMcpRecord>) -> Vec<String> {
    record
        .as_ref()
        .and_then(|r| r.enabled_in.clone())
        .unwrap_or_default()
}

/// A recorded self server that is no longer in the registry was removed on purpose.
fn removed(servers: &[ServerRecord], record: &Option<SelfMcpRecord>) -> bool {
    find_self(servers).is_none() && record.is_some()
}

fn auto_targets(view: &dyn RegistryView) -> Vec<String> {
    let mut ids: Vec<String> = view.default_access().into_iter().collect();
    for id in view.client_access_sets() {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// `Ensure` is what unattended setup runs and never overrides an opt-out. `Install` is
/// `toolportctl mcp install`, an explicit request that brings back an uninstalled server; it still
/// leaves access sets the user took it out of, and a global switch the user turned off, alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Ensure,
    Install,
}

/// Registers the server, switches it on globally when it is new, and lists it in the default
/// access set and in every set a client is scoped to, once per set. `access_set` is listed
/// regardless of earlier choices and must exist: an unknown one fails before anything is saved.
pub fn apply_install(
    tx: &mut dyn RegistryTx,
    command: &str,
    intent: Intent,
    access_set: Option<&str>,
) -> Result<Installed, String> {
    let servers = tx.servers()?;
    let mut record = tx.selfmcp_record()?;
    if removed(&servers, &record) {
        if intent == Intent::Ensure {
            return Ok(Installed {
                id: String::new(),
                outcome: Ensured::OptedOut,
                enabled: Vec::new(),
            });
        }
        tx.set_selfmcp_record(None)?;
        record = None;
    }
    if let Some(wanted) = access_set {
        if !tx.access_sets().iter().any(|set| set.id == wanted) {
            return Err(format!("access set not found: {wanted}"));
        }
    }
    let (id, outcome) = match find_self(&servers) {
        Some(existing) => {
            let current = existing.text("command") == Some(command)
                && existing.text("transport") == Some("stdio")
                && existing.text("url").is_none();
            if current {
                (existing.id.clone(), Ensured::Unchanged)
            } else {
                tx.repoint_stdio_server(&existing.id, command)?;
                (existing.id.clone(), Ensured::Updated)
            }
        }
        None if servers.iter().any(|s| s.name == SERVER_NAME) => {
            if access_set.is_some() {
                return Err(format!(
                    "a server named '{SERVER_NAME}' already exists and is not the self server"
                ));
            }
            return Ok(Installed {
                id: String::new(),
                outcome: Ensured::Unchanged,
                enabled: Vec::new(),
            });
        }
        None => {
            let id = tx.add_stdio_server(SERVER_NAME, command, SELF_SOURCE)?;
            tx.set_server_enabled(&id, true)?;
            (id, Ensured::Created)
        }
    };

    let before = enabled_in(&record);
    let mut recorded = before.clone();
    let mut enabled = Vec::new();
    for target in auto_targets(tx) {
        if recorded.contains(&target) {
            continue;
        }
        tx.set_access_server(&target, &id, true)?;
        recorded.push(target.clone());
        enabled.push(target);
    }
    if let Some(forced) = access_set {
        let listed = tx
            .access_sets()
            .iter()
            .any(|set| set.id == forced && set.server_ids.contains(&id));
        if !listed {
            tx.set_access_server(forced, &id, true)?;
            if !enabled.iter().any(|e| e == forced) {
                enabled.push(forced.to_string());
            }
        }
        if !recorded.iter().any(|r| r == forced) {
            recorded.push(forced.to_string());
        }
    }
    if recorded != before || record.is_none() {
        let mut next = record.unwrap_or_default();
        next.enabled_in = Some(recorded);
        tx.set_selfmcp_record(Some(next))?;
    }
    let outcome = match outcome {
        Ensured::Unchanged if !enabled.is_empty() => Ensured::Updated,
        other => other,
    };
    Ok(Installed {
        id,
        outcome,
        enabled,
    })
}

pub fn status(view: &dyn RegistryView) -> Result<Status, String> {
    let servers = view.servers()?;
    let record = view.selfmcp_record()?;
    let Some(entry) = find_self(&servers) else {
        return Ok(Status {
            standing: if record.is_some() {
                Standing::OptedOut
            } else {
                Standing::Missing
            },
            switched_on: false,
            default_access: None,
            client_access_sets: Vec::new(),
        });
    };
    let recorded = enabled_in(&record);
    let sets = view.access_sets();
    let state = |id: &str| {
        let member = sets
            .iter()
            .any(|set| set.id == id && set.server_ids.contains(&entry.id));
        SetState {
            id: id.to_string(),
            member,
            opted_out: !member && recorded.iter().any(|r| r == id),
        }
    };
    let default_access = view.default_access().map(|id| state(&id));
    let client_access_sets: Vec<SetState> = view
        .client_access_sets()
        .iter()
        .map(|id| state(id))
        .collect();
    let all = || default_access.iter().chain(client_access_sets.iter());
    let standing = if !entry.enabled {
        Standing::Disabled
    } else if all().any(|set| !set.member && !set.opted_out) {
        Standing::NotEnabled
    } else if all().any(|set| set.opted_out) {
        Standing::Disabled
    } else {
        Standing::Enabled
    };
    Ok(Status {
        standing,
        switched_on: entry.enabled,
        default_access,
        client_access_sets,
    })
}

pub fn install(
    host: &dyn Host,
    access_set: Option<&str>,
    intent: Intent,
) -> Result<Installed, String> {
    let command = binary_path(host);
    let mut installed = None;
    host.transact(&mut |tx| {
        installed = Some(apply_install(tx, &command, intent, access_set)?);
        Ok(())
    })?;
    installed.ok_or_else(|| "install did not run".to_string())
}

/// Removes the entry and records the opt-out, so unattended setup leaves it out.
pub fn uninstall_self_server(host: &dyn Host) -> Result<Option<String>, String> {
    let mut removed_id = None;
    host.transact(&mut |tx| {
        removed_id = None;
        if let Some(id) = find_self(&tx.servers()?).map(|s| s.id.clone()) {
            tx.remove_server(&id)?;
            removed_id = Some(id);
        }
        let mut record = tx.selfmcp_record()?.unwrap_or_default();
        record.opt_out = Some(true);
        tx.set_selfmcp_record(Some(record))
    })?;
    Ok(removed_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolport_plus_api::{AccessSetInfo, MemoryHost, MemoryRegistry, RegistryAccess};

    fn set(id: &str) -> AccessSetInfo {
        AccessSetInfo {
            id: id.into(),
            name: id.into(),
            server_ids: Vec::new(),
        }
    }

    fn host_with(sets: &[&str], default: Option<&str>, clients: &[&str]) -> MemoryHost {
        let registry = MemoryRegistry::with_servers(Vec::new());
        *registry.access_sets.borrow_mut() = sets.iter().map(|id| set(id)).collect();
        *registry.default_access.borrow_mut() = default.map(String::from);
        *registry.client_access_sets.borrow_mut() = clients.iter().map(|c| c.to_string()).collect();
        MemoryHost {
            registry,
            seam: true,
            ..Default::default()
        }
    }

    fn members(host: &MemoryHost, id: &str) -> Vec<String> {
        host.registry
            .access_sets
            .borrow()
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .server_ids
            .clone()
    }

    #[test]
    fn a_fresh_install_with_the_default_of_all_enabled_servers_lists_it_in_no_set() {
        let host = host_with(&[], None, &[]);
        let done = install(&host, None, Intent::Install).unwrap();
        assert_eq!(done.outcome, Ensured::Created);
        assert_eq!(done.id, SERVER_NAME);
        assert!(done.enabled.is_empty());
        let servers = host.registry.servers.borrow();
        assert!(servers[0].enabled);
        assert_eq!(servers[0].text("source"), Some(SELF_SOURCE));
        drop(servers);
        let again = install(&host, None, Intent::Install).unwrap();
        assert_eq!(again.outcome, Ensured::Unchanged);
    }

    #[test]
    fn the_default_set_and_the_sets_clients_use_get_it_once_each() {
        let host = host_with(
            &["focus", "wide", "idle"],
            Some("focus"),
            &["wide", "focus"],
        );
        let done = install(&host, None, Intent::Install).unwrap();
        assert_eq!(done.enabled, vec!["focus".to_string(), "wide".to_string()]);
        assert_eq!(members(&host, "focus"), vec![SERVER_NAME.to_string()]);
        assert!(members(&host, "idle").is_empty());
        let record = host.registry.plus_data().unwrap().selfmcp.unwrap();
        assert_eq!(record.enabled_in, Some(vec!["focus".into(), "wide".into()]));
    }

    #[test]
    fn a_set_the_user_took_it_out_of_stays_out_on_the_next_install() {
        let host = host_with(&["focus"], Some("focus"), &[]);
        install(&host, None, Intent::Install).unwrap();
        host.registry.access_sets.borrow_mut()[0].server_ids.clear();
        let again = install(&host, None, Intent::Install).unwrap();
        assert!(again.enabled.is_empty());
        assert!(members(&host, "focus").is_empty());
        let view = host.read_view().unwrap().unwrap();
        let status = status(&*view).unwrap();
        assert_eq!(status.standing, Standing::Disabled);
        assert!(status.default_access.unwrap().opted_out);
    }

    #[test]
    fn an_explicit_access_set_is_listed_even_after_an_earlier_choice() {
        let host = host_with(&["focus", "other"], Some("focus"), &[]);
        install(&host, None, Intent::Install).unwrap();
        host.registry.access_sets.borrow_mut()[0].server_ids.clear();
        let done = install(&host, Some("focus"), Intent::Install).unwrap();
        assert_eq!(done.enabled, vec!["focus".to_string()]);
        assert_eq!(members(&host, "focus"), vec![SERVER_NAME.to_string()]);
    }

    #[test]
    fn an_unknown_access_set_fails_before_anything_is_saved() {
        let host = host_with(&["focus"], None, &[]);
        let error = install(&host, Some("nope"), Intent::Install).unwrap_err();
        assert_eq!(error, "access set not found: nope");
        assert!(host.registry.servers.borrow().is_empty());
        assert!(host.registry.plus_data().unwrap().selfmcp.is_none());
    }

    #[test]
    fn uninstall_keeps_it_out_for_ensure_and_install_brings_it_back() {
        let host = host_with(&["focus"], Some("focus"), &[]);
        install(&host, None, Intent::Install).unwrap();
        assert_eq!(
            uninstall_self_server(&host).unwrap().as_deref(),
            Some(SERVER_NAME)
        );
        assert!(members(&host, "focus").is_empty());
        let ensured = install(&host, None, Intent::Ensure).unwrap();
        assert_eq!(ensured.outcome, Ensured::OptedOut);
        assert!(host.registry.servers.borrow().is_empty());
        let back = install(&host, None, Intent::Install).unwrap();
        assert_eq!(back.outcome, Ensured::Created);
        assert_eq!(members(&host, "focus"), vec![SERVER_NAME.to_string()]);
        assert!(uninstall_self_server(&host).unwrap().is_some());
        assert_eq!(uninstall_self_server(&host).unwrap(), None);
    }

    #[test]
    fn a_global_switch_turned_off_is_not_turned_back_on_by_install() {
        let host = host_with(&[], None, &[]);
        install(&host, None, Intent::Install).unwrap();
        host.registry
            .set_server_enabled(SERVER_NAME, false)
            .unwrap();
        install(&host, None, Intent::Install).unwrap();
        assert!(!host.registry.servers.borrow()[0].enabled);
        let view = host.read_view().unwrap().unwrap();
        assert_eq!(status(&*view).unwrap().standing, Standing::Disabled);
    }

    #[test]
    fn a_stale_command_is_repointed_and_an_unrelated_server_of_that_name_is_left_alone() {
        let host = host_with(&[], None, &[]);
        host.transact(&mut |tx| {
            tx.add_stdio_server(SERVER_NAME, "/old/toolport-selfmcp", SELF_SOURCE)
                .map(|_| ())
        })
        .unwrap();
        let done = install(&host, None, Intent::Install).unwrap();
        assert_eq!(done.outcome, Ensured::Updated);
        assert_ne!(
            host.registry.servers.borrow()[0].text("command"),
            Some("/old/toolport-selfmcp")
        );

        let other = host_with(&[], None, &[]);
        other
            .transact(&mut |tx| tx.add_stdio_server(SERVER_NAME, "x", "user").map(|_| ()))
            .unwrap();
        let done = install(&other, None, Intent::Install).unwrap();
        assert_eq!(done.id, "");
        assert_eq!(done.outcome, Ensured::Unchanged);
        assert!(install(&other, Some("none"), Intent::Install).is_err());
    }

    #[test]
    fn status_reports_missing_and_not_enabled() {
        let host = host_with(&["focus"], Some("focus"), &[]);
        let view = host.read_view().unwrap().unwrap();
        assert_eq!(status(&*view).unwrap().standing, Standing::Missing);
        host.transact(&mut |tx| {
            let id = tx.add_stdio_server(SERVER_NAME, "x", SELF_SOURCE)?;
            tx.set_server_enabled(&id, true)
        })
        .unwrap();
        let view = host.read_view().unwrap().unwrap();
        assert_eq!(status(&*view).unwrap().standing, Standing::NotEnabled);
    }
}
