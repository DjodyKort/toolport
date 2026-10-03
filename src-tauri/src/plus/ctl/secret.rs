use super::output::{CtlError, Output};
use crate::plus::registry_ro;
use serde_json::json;
use std::io::{IsTerminal, Read};

const MAX_VALUE_BYTES: u64 = 64 * 1024;

struct Target {
    server: String,
    key: String,
}

struct Options {
    reveal: bool,
    value_env: Option<String>,
    operands: Vec<String>,
}

fn parse_options(rest: &[String], allow: &[&str]) -> Result<Options, CtlError> {
    let mut options = Options {
        reveal: false,
        value_env: None,
        operands: Vec::new(),
    };
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--reveal" if allow.contains(&"--reveal") => options.reveal = true,
            "--value-env" if allow.contains(&"--value-env") => {
                let name = iter
                    .next()
                    .ok_or_else(|| CtlError::usage("--value-env requires a variable name"))?;
                options.value_env = Some(name.clone());
            }
            flag if flag.starts_with('-') && flag != "-" => {
                return Err(CtlError::usage(format!("unknown option: {flag}")));
            }
            _ => options.operands.push(arg.clone()),
        }
    }
    Ok(options)
}

fn resolve_target(operands: &[String], usage: &str, must_exist: bool) -> Result<Target, CtlError> {
    let [server, key] = operands else {
        return Err(CtlError::usage(usage));
    };
    if key.is_empty() || key.contains("::") || (key.starts_with("__") && key.ends_with("__")) {
        return Err(CtlError::usage(format!("invalid secret key: {key}")));
    }
    let registry = registry_ro::read_opt();
    let found = registry.as_ref().and_then(|reg| {
        reg.servers
            .iter()
            .find(|s| s.id == *server)
            .or_else(|| reg.servers.iter().find(|s| s.name == *server))
    });
    let server_id = match found {
        Some(entry) => entry.id.clone(),
        None if must_exist => {
            return Err(CtlError::new(
                "not_found",
                format!("unknown server: {server}"),
            ))
        }
        None => server.clone(),
    };
    Ok(Target {
        server: server_id,
        key: key.clone(),
    })
}

fn strip_line_ending(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

fn read_value(
    value_env: Option<&str>,
    reader: &mut dyn Read,
    interactive: bool,
) -> Result<String, CtlError> {
    let raw = match value_env {
        Some(name) => std::env::var(name).map_err(|_| {
            CtlError::new("input", format!("environment variable {name} is not set"))
        })?,
        None => {
            if interactive {
                return Err(CtlError::usage(
                    "pipe the value on stdin or use --value-env <VAR>",
                ));
            }
            let mut text = String::new();
            reader
                .take(MAX_VALUE_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|e| CtlError::new("input", format!("cannot read value: {e}")))?;
            if text.len() as u64 > MAX_VALUE_BYTES {
                return Err(CtlError::new("input", "value is too large"));
            }
            text
        }
    };
    let value = strip_line_ending(raw);
    if value.is_empty() {
        return Err(CtlError::new("input", "value is empty"));
    }
    Ok(value)
}

pub fn set(rest: &[String]) -> Result<Output, CtlError> {
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal();
    set_from(rest, &mut stdin.lock(), interactive)
}

fn set_from(rest: &[String], reader: &mut dyn Read, interactive: bool) -> Result<Output, CtlError> {
    let options = parse_options(rest, &["--value-env"])?;
    let target = resolve_target(
        &options.operands,
        "usage: secret set <server> <KEY> [--value-env <VAR>] (value on stdin)",
        true,
    )?;
    let value = read_value(options.value_env.as_deref(), reader, interactive)?;
    crate::secrets::set_secret(&target.server, &target.key, &value)
        .map_err(|e| CtlError::new("vault", e))?;
    Ok(Output::new(
        json!({"server": target.server, "key": target.key, "stored": true}),
        format!("Stored {} for {}", target.key, target.server),
    ))
}

pub fn get(rest: &[String]) -> Result<Output, CtlError> {
    let options = parse_options(rest, &["--reveal"])?;
    let target = resolve_target(
        &options.operands,
        "usage: secret get <server> <KEY> [--reveal]",
        false,
    )?;
    let value = crate::secrets::get_vault_secret_result(&target.server, &target.key)
        .map_err(|e| CtlError::new("vault", e))?;
    let Some(value) = value else {
        return Err(CtlError::new(
            "not_found",
            format!("{} is not set for {}", target.key, target.server),
        ));
    };
    let mut data = json!({"server": target.server, "key": target.key, "set": true});
    let human = if options.reveal {
        data["value"] = json!(value);
        value
    } else {
        format!("{} is set for {}", target.key, target.server)
    };
    Ok(Output::new(data, human))
}

pub fn rm(rest: &[String]) -> Result<Output, CtlError> {
    let options = parse_options(rest, &[])?;
    let target = resolve_target(&options.operands, "usage: secret rm <server> <KEY>", false)?;
    crate::secrets::delete_secret(&target.server, &target.key)
        .map_err(|e| CtlError::new("vault", e))?;
    Ok(Output::new(
        json!({"server": target.server, "key": target.key, "removed": true}),
        format!("Removed {} for {}", target.key, target.server),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;
    use std::io::Cursor;

    const FAKE: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn with_vault(test: impl FnOnce()) {
        crate::secrets::tests::with_isolated_vault(|| {
            let dir = registry::conduit_dir().unwrap();
            let reg = json!({
                "version": 1,
                "servers": [
                    {"id": "srv-alpha", "name": "alpha", "transport": "stdio", "command": "x", "args": []}
                ],
                "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
                "activeProfileId": "default"
            });
            std::fs::write(dir.join("registry.json"), reg.to_string()).unwrap();
            test();
        });
    }

    fn stdin(text: &str) -> Cursor<Vec<u8>> {
        Cursor::new(text.as_bytes().to_vec())
    }

    #[test]
    fn set_get_rm_round_trip_through_file_vault() {
        with_vault(|| {
            let out = set_from(
                &args(&["alpha", "API_KEY"]),
                &mut stdin(&format!("{FAKE}\n")),
                false,
            )
            .unwrap();
            assert_eq!(out.data["server"], "srv-alpha");
            assert!(!out.human.contains(FAKE));
            assert!(!out.data.to_string().contains(FAKE));

            let masked = get(&args(&["srv-alpha", "API_KEY"])).unwrap();
            assert_eq!(masked.data["set"], true);
            assert!(masked.data.get("value").is_none());
            assert!(!masked.human.contains(FAKE));
            assert!(!masked.data.to_string().contains(FAKE));

            let shown = get(&args(&["srv-alpha", "API_KEY", "--reveal"])).unwrap();
            assert_eq!(shown.human, FAKE);
            assert_eq!(shown.data["value"], FAKE);

            rm(&args(&["alpha", "API_KEY"])).unwrap();
            let err = get(&args(&["alpha", "API_KEY"])).err().unwrap();
            assert_eq!(err.code, "not_found");
        });
    }

    #[test]
    fn value_is_stored_encrypted_not_in_registry_or_plaintext() {
        with_vault(|| {
            set_from(&args(&["alpha", "TOKEN"]), &mut stdin(FAKE), false).unwrap();
            let dir = registry::conduit_dir().unwrap();
            for entry in std::fs::read_dir(&dir).unwrap() {
                let bytes = std::fs::read(entry.unwrap().path()).unwrap();
                let text = String::from_utf8_lossy(&bytes);
                assert!(!text.contains(FAKE));
            }
            assert!(dir.join("secrets.enc").exists());
        });
    }

    #[test]
    fn value_env_source_and_line_ending_handling() {
        with_vault(|| {
            std::env::set_var("FAKE_CTL_VALUE", format!("{FAKE}\r\n"));
            set_from(
                &args(&["alpha", "K", "--value-env", "FAKE_CTL_VALUE"]),
                &mut stdin(""),
                false,
            )
            .unwrap();
            std::env::remove_var("FAKE_CTL_VALUE");
            let shown = get(&args(&["alpha", "K", "--reveal"])).unwrap();
            assert_eq!(shown.data["value"], FAKE);

            set_from(&args(&["alpha", "K2"]), &mut stdin("a\nb\n"), false).unwrap();
            let shown = get(&args(&["alpha", "K2", "--reveal"])).unwrap();
            assert_eq!(shown.data["value"], "a\nb");
        });
    }

    #[test]
    fn rejects_bad_input_without_storing() {
        with_vault(|| {
            let cases: Vec<(Vec<String>, &str, &str)> = vec![
                (args(&["alpha"]), "x", "usage"),
                (args(&["alpha", "__http_auth__"]), "x", "usage"),
                (args(&["alpha", "A::B"]), "x", "usage"),
                (args(&["alpha", "--bogus"]), "x", "usage"),
                (args(&["ghost", "K"]), "x", "not_found"),
                (args(&["alpha", "K"]), "", "input"),
                (args(&["alpha", "K"]), "\n", "input"),
            ];
            for (list, input, code) in cases {
                let err = set_from(&list, &mut stdin(input), false).err().unwrap();
                assert_eq!(err.code, code, "{list:?}");
            }
            let err = set_from(&args(&["alpha", "K"]), &mut stdin(FAKE), true)
                .err()
                .unwrap();
            assert_eq!(err.code, "usage");
            let err = set_from(
                &args(&["alpha", "K", "--value-env", "FAKE_CTL_UNSET_VAR"]),
                &mut stdin(""),
                false,
            )
            .err()
            .unwrap();
            assert_eq!(err.code, "input");
            assert_eq!(get(&args(&["alpha", "K"])).err().unwrap().code, "not_found");
        });
    }

    #[test]
    fn get_and_rm_accept_unregistered_server_ids() {
        with_vault(|| {
            assert_eq!(
                get(&args(&["orphan", "K"])).err().unwrap().code,
                "not_found"
            );
            assert!(rm(&args(&["orphan", "K"])).is_ok());
        });
    }

    #[test]
    fn cli_wiring_and_json_envelope_never_leak_value() {
        with_vault(|| {
            set_from(&args(&["alpha", "API_KEY"]), &mut stdin(FAKE), false).unwrap();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = super::super::run_with(
                &args(&["--json", "secret", "get", "alpha", "API_KEY"]),
                &mut out,
                &mut err,
            );
            let text = String::from_utf8(out).unwrap();
            assert_eq!(code, 0);
            assert!(!text.contains(FAKE));
            let value: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
            assert_eq!(value["command"], "secret get");
            assert_eq!(value["data"]["set"], true);

            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = super::super::run_with(
                &args(&["secret", "get", "alpha", "MISSING"]),
                &mut out,
                &mut err,
            );
            assert_eq!(code, 1);
            assert!(out.is_empty());
            assert!(String::from_utf8(err).unwrap().contains("not set"));

            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code =
                super::super::run_with(&args(&["secret", "get", "alpha"]), &mut out, &mut err);
            assert_eq!(code, 2);
        });
    }
}
