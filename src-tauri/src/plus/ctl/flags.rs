//! The one flag parser behind every `toolportctl` command. A command declares its flags as a
//! `Spec` const; the style fields keep the wording and edge cases each command has always had.

use super::output::CtlError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Switch,
    Value,
}

#[derive(Clone, Copy)]
pub(super) struct Flag {
    name: &'static str,
    aliases: &'static [&'static str],
    kind: Kind,
    needs: &'static str,
}

pub(super) const fn switch(name: &'static str) -> Flag {
    Flag::new(name, Kind::Switch)
}

pub(super) const fn value(name: &'static str) -> Flag {
    Flag::new(name, Kind::Value)
}

impl Flag {
    const fn new(name: &'static str, kind: Kind) -> Self {
        Self {
            name,
            aliases: &[],
            kind,
            needs: "a value",
        }
    }

    pub(super) const fn alias(self, aliases: &'static [&'static str]) -> Self {
        Self { aliases, ..self }
    }

    pub(super) const fn needs(self, what: &'static str) -> Self {
        Self { needs: what, ..self }
    }

    fn matches(&self, key: &str) -> bool {
        self.name == key || self.aliases.contains(&key)
    }
}

/// Whether `--flag=value` is understood. `Strict` also rejects a value on a switch with
/// "takes no value"; the others treat that token as unknown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Inline {
    Off,
    Value,
    Strict,
}

/// The error for a token the spec does not declare.
#[derive(Clone, Copy)]
pub(super) enum Unknown {
    Option,
    Argument,
    ArgumentUsage(&'static str),
}

#[derive(Clone, Copy)]
pub(super) enum Operands {
    Collect,
    Reject,
}

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub flags: &'static [Flag],
    pub inline: Inline,
    pub unknown: Unknown,
    pub operands: Operands,
}

#[derive(Default)]
pub(super) struct Flags {
    values: Vec<(&'static str, String)>,
    switches: Vec<&'static str>,
    operands: Vec<String>,
}

impl Spec {
    pub(super) const PLAIN: Spec = Spec {
        flags: &[],
        inline: Inline::Off,
        unknown: Unknown::Option,
        operands: Operands::Collect,
    };

    fn unknown(&self, arg: &str, key: &str) -> CtlError {
        CtlError::usage(match self.unknown {
            Unknown::Option => format!("unknown option: {key}"),
            Unknown::Argument => format!("unexpected argument: {arg}"),
            Unknown::ArgumentUsage(usage) => format!("unexpected argument: {arg}\n{usage}"),
        })
    }

    pub(super) fn parse(&self, rest: &[String]) -> Result<Flags, CtlError> {
        let mut out = Flags::default();
        let mut iter = rest.iter();
        while let Some(arg) = iter.next() {
            if !arg.starts_with('-') || arg == "-" {
                match self.operands {
                    Operands::Collect => out.operands.push(arg.clone()),
                    Operands::Reject => return Err(self.unknown(arg, arg)),
                }
                continue;
            }
            let (key, inline) = match arg.split_once('=') {
                Some((key, value)) if self.inline != Inline::Off => (key, Some(value)),
                _ => (arg.as_str(), None),
            };
            let Some(flag) = self.flags.iter().find(|f| f.matches(key)) else {
                return Err(self.unknown(arg, key));
            };
            if flag.kind == Kind::Switch {
                match (inline, self.inline) {
                    (None, _) => out.switches.push(flag.name),
                    (Some(_), Inline::Strict) => {
                        return Err(CtlError::usage(format!("{} takes no value", flag.name)))
                    }
                    (Some(_), _) => return Err(self.unknown(arg, key)),
                }
                continue;
            }
            let value = match inline {
                Some(value) => value.to_string(),
                None => iter.next().cloned().ok_or_else(|| {
                    CtlError::usage(format!("{} requires {}", flag.name, flag.needs))
                })?,
            };
            out.values.push((flag.name, value));
        }
        Ok(out)
    }
}

impl Flags {
    pub(super) fn one(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    pub(super) fn all(&self, name: &str) -> Vec<String> {
        self.values
            .iter()
            .filter(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }

    pub(super) fn on(&self, name: &str) -> bool {
        self.switches.contains(&name)
    }

    pub(super) fn has_values(&self) -> bool {
        !self.values.is_empty()
    }

    pub(super) fn operands(&self) -> &[String] {
        &self.operands
    }

    pub(super) fn single(&self, usage: &str) -> Result<&str, CtlError> {
        match self.operands.as_slice() {
            [one] => Ok(one),
            _ => Err(CtlError::usage(usage)),
        }
    }
}
