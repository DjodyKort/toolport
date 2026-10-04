//! `toolportctl profile ls|create|edit|rm`: renderers over `plus::profiles`. Texts follow mcpm's
//! `profile` group with `toolportctl` in the hints (D-042); `--force` is accepted where mcpm has
//! it, because the ctl never asks for a confirmation.

use super::flags::{switch, value, Spec};
use super::output::{table, CtlError, Output};
use super::skills_repo::{no_operands, operand, spec};
use crate::plus::profiles::{self, EditSpec, Error, Kind, ServerOp};

const LS_USAGE: &str = "usage: profile ls [--verbose|-v]";
const CREATE_USAGE: &str = "usage: profile create <name> [--force] [--dry-run]";
const EDIT_USAGE: &str = "usage: profile edit <profile> [--name <new>] [--servers <a,b>] \
     [--set-servers <a,b>] [--add-server <a,b>] [--remove-server <a,b>] [--force] [--dry-run]";
const RM_USAGE: &str = "usage: profile rm <profile> [--no-clients] [--force|-f] [--dry-run]";

pub const GROUP_USAGE: &str = "usage: profile ls|create|edit|rm|inspect (ls: [--verbose|-v]; \
     create: <name> [--force] [--dry-run]; edit: <profile> [--name <new>] [--servers <a,b>] \
     [--set-servers <a,b>] [--add-server <a,b>] [--remove-server <a,b>] [--force] [--dry-run]; \
     rm: <profile> [--no-clients] [--force|-f] [--dry-run]; inspect: [<profile>])";

pub(super) const LS: Spec = spec(&[switch("--verbose").alias(&["-v"])], LS_USAGE);
pub(super) const CREATE: Spec = spec(&[switch("--force"), switch("--dry-run")], CREATE_USAGE);
pub(super) const EDIT: Spec = spec(
    &[
        value("--name"),
        value("--servers"),
        value("--set-servers"),
        value("--add-server"),
        value("--remove-server"),
        switch("--force"),
        switch("--dry-run"),
    ],
    EDIT_USAGE,
);
pub(super) const RM: Spec = spec(
    &[
        switch("--force").alias(&["-f"]),
        switch("--no-clients"),
        switch("--dry-run"),
    ],
    RM_USAGE,
);

const DRY_NOTE: &str = "Dry run: nothing was written.";

pub(super) fn ctl_error(error: Error) -> CtlError {
    match error.kind {
        Kind::Invalid => CtlError::usage(error.message),
        Kind::NotFound => CtlError::not_found(error.message),
        Kind::Conflict => CtlError::conflict(error.message),
        Kind::Failed(code) => CtlError::failed(code, error.message),
    }
}

pub(super) fn list_of(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(GROUP_USAGE))
}

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    no_operands(&flags, LS_USAGE)?;
    let rows = profiles::list().map_err(ctl_error)?;
    let data = profiles::list_value(&rows);
    if rows.is_empty() {
        return Ok(Output::new(data, "No profiles found.".to_string()));
    }
    let verbose = flags.on("--verbose");
    let mut headers = vec!["Name", "Servers"];
    if verbose {
        headers.push("Server Details");
    }
    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let mut cells = vec![
                row.name.clone(),
                row.servers
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ];
            if verbose {
                cells.push(
                    row.servers
                        .iter()
                        .map(|m| format!("{}: {}", m.name, m.target))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            cells
        })
        .collect();
    let active = rows
        .iter()
        .find(|r| r.active)
        .map_or("-", |r| r.name.as_str());
    let human = format!(
        "Found {} profile(s)\n\n{}\nActive profile: {active}",
        rows.len(),
        table(&headers, &body)
    );
    Ok(Output::new(data, human))
}

pub fn create(rest: &[String]) -> Result<Output, CtlError> {
    let flags = CREATE.parse(rest)?;
    let name = operand(&flags, "profile name", CREATE_USAGE)?;
    let dry = flags.on("--dry-run");
    let made = profiles::create(name, flags.on("--force"), dry).map_err(ctl_error)?;
    let human = match (made.created, dry) {
        (true, true) => format!("Would create profile '{}'.\n{DRY_NOTE}", made.name),
        (true, false) => format!(
            "Profile '{}' created successfully.\n\nYou can now edit this profile to add servers using 'toolportctl profile edit {}'",
            made.name, made.id
        ),
        (false, true) => format!("Profile '{}' already exists; nothing would change.\n{DRY_NOTE}", made.name),
        (false, false) => format!("Profile '{}' already exists; nothing changed.", made.name),
    };
    Ok(Output::new(made.to_value(), human))
}

fn edit_text(edited: &profiles::Edited, dry: bool) -> String {
    let mut lines = Vec::new();
    if !edited.not_in_profile.is_empty() {
        lines.push(format!(
            "Warning: Server(s) not in profile: {}",
            edited.not_in_profile.join(", ")
        ));
        lines.push(String::new());
    }
    lines.push(format!("Updating profile '{}':", edited.old_name));
    if !edited.changed() {
        lines.push("No changes specified".to_string());
        return lines.join("\n");
    }
    if edited.renamed() {
        lines.push(format!("Name: {} → {}", edited.old_name, edited.name));
    }
    let (added, removed) = (sorted(edited.added()), sorted(edited.removed()));
    if !added.is_empty() || !removed.is_empty() {
        lines.push(format!(
            "Servers: {} servers → {} servers",
            edited.before.len(),
            edited.after.len()
        ));
        if !added.is_empty() {
            lines.push(format!("  + Added: {}", added.join(", ")));
        }
        if !removed.is_empty() {
            lines.push(format!("  - Removed: {}", removed.join(", ")));
        }
    }
    lines.push(String::new());
    if dry {
        lines.push(DRY_NOTE.to_string());
        return lines.join("\n");
    }
    lines.push("Applying changes...".to_string());
    lines.push(if edited.renamed() {
        format!(
            "✅ Profile renamed from '{}' to '{}'",
            edited.old_name, edited.name
        )
    } else {
        format!("✅ Profile '{}' updated", edited.old_name)
    });
    lines.push(format!(
        "✅ {} servers configured in profile",
        edited.after.len()
    ));
    lines.join("\n")
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

pub fn edit(rest: &[String]) -> Result<Output, CtlError> {
    let flags = EDIT.parse(rest)?;
    let key = operand(&flags, "profile", EDIT_USAGE)?;
    let given: Vec<ServerOp> = [
        flags.one("--servers").map(|v| ServerOp::Set(list_of(v))),
        flags
            .one("--set-servers")
            .map(|v| ServerOp::Set(list_of(v))),
        flags.one("--add-server").map(|v| ServerOp::Add(list_of(v))),
        flags
            .one("--remove-server")
            .map(|v| ServerOp::Remove(list_of(v))),
    ]
    .into_iter()
    .flatten()
    .collect();
    if given.len() > 1 {
        return Err(CtlError::usage(
            "Cannot use multiple server options simultaneously\nUse either --servers, --add-server, --remove-server, or --set-servers",
        ));
    }
    let dry = flags.on("--dry-run");
    let edit = EditSpec {
        name: flags.one("--name").map(String::from),
        servers: given.into_iter().next(),
    };
    let edited = profiles::edit(key, &edit, dry).map_err(ctl_error)?;
    let human = edit_text(&edited, dry);
    Ok(Output::new(edited.to_value(), human))
}

fn rm_text(removed: &profiles::Removed, dry: bool) -> String {
    let mut lines = Vec::new();
    if dry {
        lines.push(format!("Would remove profile '{}'.", removed.name));
    } else {
        lines.push(format!(
            "✅ Profile '{}' removed successfully",
            removed.name
        ));
    }
    if removed.servers > 0 {
        lines.push(format!(
            "{} server(s) {} in the registry",
            removed.servers,
            if dry { "would remain" } else { "remain" }
        ));
    }
    let cleaned: Vec<_> = removed
        .cleanups
        .iter()
        .filter(|c| c.error.is_none())
        .collect();
    if !cleaned.is_empty() {
        let (n, word) = (cleaned.len(), if cleaned.len() == 1 { "y" } else { "ies" });
        lines.push(if dry {
            format!("Would remove {n} entr{word} from {n} client(s):")
        } else {
            format!("Cleaned {n} entr{word} from {n} client(s):")
        });
        lines.extend(
            cleaned
                .iter()
                .map(|c| format!("  • {}: gateway entry", c.name)),
        );
        if !dry {
            lines.push("Restart your MCP clients for the changes to take effect.".to_string());
        }
    }
    for failed in removed.cleanups.iter().filter(|c| c.error.is_some()) {
        lines.push(format!(
            "Could not clean {}: {}",
            failed.name,
            failed.error.as_deref().unwrap_or("")
        ));
    }
    if !removed.left.is_empty() {
        lines.push(format!(
            "Left in place: {} (still scoped to the removed profile, they see no servers until re-scoped: toolportctl client edit <id> --set-profiles <profile>)",
            removed.left.join(", ")
        ));
    }
    if dry {
        lines.push(DRY_NOTE.to_string());
    }
    lines.join("\n")
}

pub fn rm(rest: &[String]) -> Result<Output, CtlError> {
    let flags = RM.parse(rest)?;
    let key = operand(&flags, "profile", RM_USAGE)?;
    let dry = flags.on("--dry-run");
    let removed = profiles::remove(key, flags.on("--no-clients"), dry).map_err(ctl_error)?;
    let human = rm_text(&removed, dry);
    let mut output = Output::new(removed.to_value(), human);
    output.failed = removed.failed();
    Ok(output)
}
