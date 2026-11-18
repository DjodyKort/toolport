use toolport_plus_api::{Host, ServerRecord};

use super::access;
use super::feed_access::AccessInputs;
use super::feed_credentials::{CredentialInputs, ServerFacts};
use super::feed_library::LibraryInputs;
use super::feed_tasks::{TaskFacts, TaskInputs};
use super::feed_updates::UpdateInputs;
use super::store::Store;
use super::updates;
use crate::auth::{self, AuthStore};
use crate::library::remote;
use crate::skills::auto;
use crate::tasks::schedule::read_ledger;
use crate::tasks::store;

/// Environment keys a server declares as secret and keeps no inline value for: the ones the
/// vault has to hold.
fn secret_keys(server: &ServerRecord) -> Vec<String> {
    server
        .env()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.secret && entry.value.is_none())
        .map(|entry| entry.key)
        .collect()
}

pub fn credentials(host: &dyn Host) -> Result<CredentialInputs, String> {
    let all = auth::read_servers(host).map_err(|e| format!("the registry cannot be read: {e}"))?;
    let mut vault_error = None;
    let servers = all
        .iter()
        .map(|server| {
            let mut missing = Vec::new();
            if server.enabled && vault_error.is_none() {
                for key in secret_keys(server) {
                    match host.read_vault_secret(&server.id, &key) {
                        Ok(Some(value)) if !value.trim().is_empty() => {}
                        Ok(_) => missing.push(key),
                        Err(error) => {
                            vault_error = Some(error);
                            break;
                        }
                    }
                }
            }
            ServerFacts {
                id: server.id.clone(),
                name: server.name.clone(),
                enabled: server.enabled,
                missing_secrets: missing,
            }
        })
        .collect();
    let status = auth::auth_dir(host)
        .map(|dir| AuthStore::new(&dir).read_status())
        .unwrap_or_default();
    let logins = auth::rows_for(
        &status,
        host.now_unix(),
        &auth::visible_servers(host, &status),
    );
    Ok(CredentialInputs {
        servers,
        vault_error,
        logins,
    })
}

pub fn access(host: &dyn Host, store: Option<&Store>) -> Result<AccessInputs, String> {
    access::inputs(host, store)
}

pub fn tasks(host: &dyn Host) -> Result<TaskInputs, String> {
    let base = host
        .tasks_base()
        .ok_or_else(|| "the data directory could not be resolved".to_string())?;
    let tasks = store::list_tasks(&base)
        .into_iter()
        .filter_map(Result::ok)
        .map(|task| TaskFacts {
            id: task.id,
            title: task.title,
            enabled: task.enabled,
        })
        .collect();
    Ok(TaskInputs {
        tasks,
        runs: store::list_runs(&base, None),
        missed: read_ledger(&base).missed,
        records: store::attention(&base),
    })
}

pub fn library(host: &dyn Host) -> Result<LibraryInputs, String> {
    let base = host
        .data_dir()
        .ok_or_else(|| "the data directory could not be resolved".to_string())?;
    let _paths = crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: Some(base.clone()),
    });
    Ok(LibraryInputs {
        auto: auto::read(&base).map_err(|e| e.message),
        repo: remote::locate().ok().map(|repo| remote::local_state(&repo)),
    })
}

pub fn store(host: &dyn Host) -> Result<Store, String> {
    let base = host
        .data_dir()
        .ok_or_else(|| "the data directory could not be resolved".to_string())?;
    Store::read(&base)
}

pub fn updates(host: &dyn Host, store: Option<&Store>) -> Result<UpdateInputs, String> {
    let base = host
        .data_dir()
        .ok_or_else(|| "the data directory could not be resolved".to_string())?;
    let base = crate::skills::repo::resolve_path(&base);
    let _paths = crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: Some(base.clone()),
    });
    let mut inputs = updates::inputs(&base, store.unwrap_or(&Store::default()));
    let repos = crate::servers::repo::repos(host).unwrap_or_default();
    inputs.servers = repos.iter().map(updates::repo_facts).collect();
    let servers = crate::servers::read_servers(host).unwrap_or_default();
    inputs.packages = updates::package_facts(
        &crate::servers::package::pins(&servers),
        store.unwrap_or(&Store::default()),
    );
    if crate::sync::publishing(host) {
        let peers = crate::sync::peers::Peers::read(&host.data_dir().unwrap_or(base.clone()));
        inputs.peers = crate::servers::repo::peers::views(&repos, &peers);
        inputs.unpublished = repos
            .iter()
            .filter_map(|repo| {
                let commits = crate::servers::repo::peers::unpublished(repo);
                Some(super::feed_repo_heads::Unpublished {
                    server: repo.first_name().to_string(),
                    head: repo.state.head.clone()?,
                    commits,
                })
                .filter(|u| u.commits > 0)
            })
            .collect();
    }
    Ok(inputs)
}
