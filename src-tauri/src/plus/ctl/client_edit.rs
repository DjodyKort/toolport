//! `toolportctl client edit|import`: renderers over `plus::profiles::client`. mcpm's profile
//! options map onto the one profile a client's gateway entry follows; its server options do not
//! (servers reach a client through the gateway and its profile), and `client import` reads direct
//! entries into the registry without interaction.

use super::flags::{switch, value, Spec};
use super::output::{table, CtlError, Output};
use super::profile::{ctl_error, list_of};
use super::skills_repo::{operand, spec};
use crate::plus::profiles::client::{self, ClientEdited, ImportSpec, Imported, ProfileOp};

const EDIT_USAGE: &str = "usage: client edit <client> [--add-profile <p>] [--remove-profile <p>] \
     [--set-profiles <p>] [--force] [--dry-run]";
const IMPORT_USAGE: &str = "usage: client import <client> [--select <a,b> | --all] \
     [--profile <name>] [--dry-run]";

pub const GROUP_USAGE: &str =
    "usage: client ls|edit|import|sync (edit: <client> [--add-profile <p>] \
     [--remove-profile <p>] [--set-profiles <p>] [--force] [--dry-run]; import: <client> \
     [--select <a,b> | --all] [--profile <name>] [--dry-run]; sync: [--client <id>] [--dry-run] \
     [--keep-orphans])";

const EDIT: Spec = spec(
    &[
        value("--add-profile"),
        value("--remove-profile"),
        value("--set-profiles"),
        value("--add-server"),
        value("--remove-server"),
        value("--set-servers"),
        value("--file").alias(&["-f"]),
        value("--disabled"),
        switch("--external").alias(&["-e"]),
        switch("--force"),
        switch("--dry-run"),
    ],
    EDIT_USAGE,
);
const IMPORT: Spec = spec(
    &[
        value("--select"),
        value("--profile"),
        switch("--all"),
        switch("--dry-run"),
    ],
    IMPORT_USAGE,
);

const DRY_NOTE: &str = "Dry run: nothing was written.";

const SERVER_OPTIONS: [&str; 6] = [
    "--add-server",
    "--remove-server",
    "--set-servers",
    "--disabled",
    "--external",
    "--file",
];

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

fn edit_text(edited: &ClientEdited, dry: bool) -> String {
    let name = &edited.name;
    let mut lines = vec![
        format!("{name} Configuration Management"),
        format!("Config file: {}", edited.path),
        String::new(),
    ];
    if !edited.not_in_client.is_empty() {
        lines.push(format!(
            "Warning: Profile(s) not in client: {}",
            edited.not_in_client.join(", ")
        ));
    }
    lines.push(String::new());
    lines.push(format!("Updating {name} configuration:"));
    if !edited.changed() {
        lines.push("No changes specified".to_string());
        return lines.join("\n");
    }
    lines.push(format!(
        "Profiles: {} profiles → {} profiles",
        edited.before.len(),
        edited.after.len()
    ));
    let (added, removed) = (sorted(edited.added()), sorted(edited.removed()));
    if !added.is_empty() {
        lines.push(format!("  + Added: {}", added.join(", ")));
    }
    if !removed.is_empty() {
        lines.push(format!("  - Removed: {}", removed.join(", ")));
    }
    lines.push(String::new());
    if dry {
        lines.push(DRY_NOTE.to_string());
        return lines.join("\n");
    }
    lines.push("Applying changes...".to_string());
    lines.push(format!("✅ Successfully updated {name} configuration"));
    lines.push(match (&edited.after.first(), &edited.follows_active) {
        (Some(profile), _) => format!("✅ {name} follows profile '{profile}'"),
        (None, Some(active)) => format!("✅ {name} follows the active profile ('{active}')"),
        (None, None) => format!("✅ {name} follows no profile"),
    });
    lines.push(format!("Restart {name} for changes to take effect."));
    lines.join("\n")
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn edit(rest: &[String]) -> Result<Output, CtlError> {
    let flags = EDIT.parse(rest)?;
    let key = operand(&flags, "client", EDIT_USAGE)?;
    for option in SERVER_OPTIONS {
        if flags.on(option) || flags.one(option).is_some() {
            return Err(CtlError::usage(format!(
                "{option} has no counterpart: a client reaches servers through the Toolport gateway and the profile it follows\n\
                 Use `toolportctl profile edit <profile> --add-server <name>` to change what a profile serves"
            )));
        }
    }
    let given = |name: &str| flags.one(name).map(list_of).filter(|l| !l.is_empty());
    let ops: Vec<ProfileOp> = [
        given("--add-profile").map(ProfileOp::Add),
        given("--remove-profile").map(ProfileOp::Remove),
        given("--set-profiles").map(ProfileOp::Set),
    ]
    .into_iter()
    .flatten()
    .collect();
    if ops.len() > 1 {
        return Err(CtlError::usage(
            "Cannot use multiple profile options simultaneously\nUse either --add-profile, --remove-profile, or --set-profiles",
        ));
    }
    let dry = flags.on("--dry-run");
    let edited = client::edit(key, ops.first(), flags.on("--force"), dry).map_err(ctl_error)?;
    let human = edit_text(&edited, dry);
    Ok(Output::new(edited.to_value(), human))
}

fn clip(text: &str, at: usize) -> String {
    if text.chars().count() > at {
        format!("{}...", text.chars().take(at).collect::<String>())
    } else {
        text.to_string()
    }
}

fn import_text(done: &Imported, id: &str, dry: bool) -> String {
    let name = &done.name;
    let mut lines = vec![
        format!("{name} Configuration Import"),
        format!("Config file: {}", done.path),
        String::new(),
        format!(
            "Found {} server(s) in {name} configuration:",
            done.gateway.len() + done.direct.len()
        ),
        format!("  Toolport gateway entries: {}", done.gateway.len()),
        format!("  Direct servers: {}", done.direct.len()),
        String::new(),
    ];
    if !done.gateway.is_empty() {
        lines.push("Toolport gateway entries:".to_string());
        lines.extend(done.gateway.iter().map(|g| format!("  {g}")));
        lines.push(String::new());
    }
    if done.direct.is_empty() {
        lines.push("No direct servers found to import.".to_string());
        return lines.join("\n");
    }
    if !done.selected {
        lines.push("Direct servers available for import:".to_string());
        for server in &done.direct {
            let note = match server.status {
                "already" => " (already in the registry)",
                "name-taken" => " (a different server with this name is installed)",
                "credential" => " (holds an inline credential, not imported)",
                _ => "",
            };
            lines.push(format!(
                "  {} - {}{note}",
                server.name,
                clip(&server.target, 50)
            ));
        }
        lines.push(String::new());
        lines.push(format!(
            "Choose with --select <name>[,<name>...] or import everything with --all: toolportctl client import {id} --all"
        ));
        return lines.join("\n");
    }
    let count = done.imported.len() + done.skipped.len();
    lines.push(format!(
        "{} {count} server(s) {} the registry...",
        if dry { "Would import" } else { "Importing" },
        if dry { "into" } else { "to" }
    ));
    let rows: Vec<Vec<String>> = done
        .imported
        .iter()
        .map(|(_, server)| (server, "✅ Imported".to_string()))
        .chain(
            done.skipped
                .iter()
                .map(|(server, why)| (server, format!("skipped: {why}"))),
        )
        .map(|(server, status)| {
            let target = done
                .direct
                .iter()
                .find(|d| &d.name == server)
                .map_or(String::new(), |d| clip(&d.target, 30));
            vec![server.clone(), target, status]
        })
        .collect();
    lines.push(table(&["Server Name", "Command", "Status"], &rows));
    lines.push(String::new());
    lines.push(format!(
        "{} {} server(s) from {id}.",
        if dry {
            "Would import"
        } else {
            "Successfully imported"
        },
        done.imported.len()
    ));
    if done
        .skipped
        .iter()
        .any(|(_, why)| *why == client::CREDENTIAL)
    {
        lines.push(client::CREDENTIAL_HINT.to_string());
    }
    if let Some(profile) = &done.profile {
        if profile.created {
            lines.push(format!(
                "Profile '{}' {} with {} server(s).",
                profile.name,
                if dry { "would be created" } else { "created" },
                profile.servers.len()
            ));
        } else {
            lines.push(format!(
                "Profile '{}' {} {} server(s).",
                profile.name,
                if dry { "would enable" } else { "now enables" },
                profile.servers.len()
            ));
        }
    }
    if !dry && !done.imported.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "Next: toolportctl client sync --client {id}  (replaces the imported direct entries with the gateway entry)"
        ));
        for (server, key) in &done.secrets {
            lines.push(format!(
                "      toolportctl secret set {server} {key}  (the value was not copied)"
            ));
        }
    }
    if dry {
        lines.push(DRY_NOTE.to_string());
    }
    lines.join("\n")
}

pub fn import(rest: &[String]) -> Result<Output, CtlError> {
    let flags = IMPORT.parse(rest)?;
    let key = operand(&flags, "client", IMPORT_USAGE)?;
    let spec = ImportSpec {
        select: flags.one("--select").map(list_of).unwrap_or_default(),
        all: flags.on("--all"),
        profile: flags.one("--profile").map(String::from),
    };
    if spec.all && flags.one("--select").is_some() {
        return Err(CtlError::usage("Use either --select or --all, not both"));
    }
    if flags.one("--select").is_some() && spec.select.is_empty() {
        return Err(CtlError::usage(format!(
            "--select needs at least one server name\n{IMPORT_USAGE}"
        )));
    }
    let dry = flags.on("--dry-run");
    let done = client::import(key, &spec, dry).map_err(ctl_error)?;
    let human = import_text(&done, key, dry);
    Ok(Output::new(done.to_value(), human))
}
