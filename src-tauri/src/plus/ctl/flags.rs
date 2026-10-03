//! The one flag parser behind every `toolportctl` command. A command declares its flags as a
//! `Spec` const; the style fields keep the wording and edge cases each command has always had.

use super::output::CtlError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Switch,
    Value,
    Greedy,
}

#[derive(Clone, Copy)]
pub(super) struct Flag {
    name: &'static str,
    aliases: &'static [&'static str],
    kind: Kind,
    needs: &'static str,
    count: Option<&'static str>,
    nonempty: bool,
}

pub(super) const fn switch(name: &'static str) -> Flag {
    Flag::new(name, Kind::Switch)
}

pub(super) const fn value(name: &'static str) -> Flag {
    Flag::new(name, Kind::Value)
}

pub(super) const fn greedy(name: &'static str) -> Flag {
    Flag::new(name, Kind::Greedy)
}

impl Flag {
    const fn new(name: &'static str, kind: Kind) -> Self {
        Self {
            name,
            aliases: &[],
            kind,
            needs: "a value",
            count: None,
            nonempty: false,
        }
    }

    pub(super) const fn alias(self, aliases: &'static [&'static str]) -> Self {
        Self { aliases, ..self }
    }

    pub(super) const fn needs(self, what: &'static str) -> Self {
        Self { needs: what, ..self }
    }

    pub(super) const fn count(self, bad: &'static str) -> Self {
        Self {
            count: Some(bad),
            ..self
        }
    }

    pub(super) const fn nonempty(self) -> Self {
        Self {
            nonempty: true,
            ..self
        }
    }

    fn matches(&self, key: &str) -> bool {
        self.name == key || self.aliases.contains(&key)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Inline {
    Off,
    Value,
    Strict,
}

#[derive(Clone, Copy)]
pub(super) enum Unknown {
    Option,
    Argument,
    ArgumentKey,
    ArgumentUsage(&'static str),
    Named,
    NamedUsage(&'static str),
    Usage(&'static str),
}

#[derive(Clone, Copy)]
pub(super) enum Operands {
    Collect,
    Reject,
    Max(usize, &'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dashes {
    NotBare,
    Any,
    Long,
}

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub flags: &'static [Flag],
    pub inline: Inline,
    pub unknown: Unknown,
    pub operands: Operands,
    pub dashes: Dashes,
}

#[derive(Default)]
pub(super) struct Flags {
    values: Vec<(&'static str, String)>,
    switches: Vec<&'static str>,
    operands: Vec<String>,
}

impl Spec {
    pub(super) const NONE: Spec = Spec {
        flags: &[],
        unknown: Unknown::Named,
        operands: Operands::Reject,
        ..Spec::PLAIN
    };

    pub(super) const PLAIN: Spec = Spec {
        flags: &[],
        inline: Inline::Off,
        unknown: Unknown::Option,
        operands: Operands::Collect,
        dashes: Dashes::NotBare,
    };

    fn unknown(&self, arg: &str, key: &str) -> CtlError {
        CtlError::usage(match self.unknown {
            Unknown::Option => format!("unknown option: {key}"),
            Unknown::Argument => format!("unexpected argument: {arg}"),
            Unknown::ArgumentKey => format!("unexpected argument: {key}"),
            Unknown::ArgumentUsage(usage) => format!("unexpected argument: {arg}\n{usage}"),
            Unknown::Named => format!("unknown argument: {arg}"),
            Unknown::NamedUsage(usage) => format!("unknown argument: {arg}\n{usage}"),
            Unknown::Usage(usage) => usage.to_string(),
        })
    }

    pub(super) fn parse(&self, rest: &[String]) -> Result<Flags, CtlError> {
        let mut out = Flags::default();
        let mut iter = rest.iter().peekable();
        while let Some(arg) = iter.next() {
            let (key, inline) = match arg.split_once('=') {
                Some((key, value)) if self.inline != Inline::Off => (key, Some(value)),
                _ => (arg.as_str(), None),
            };
            let flaglike = match self.dashes {
                Dashes::NotBare => arg.starts_with('-') && arg != "-",
                Dashes::Any => arg.starts_with('-'),
                Dashes::Long => arg.starts_with("--"),
            };
            if !flaglike {
                match self.operands {
                    Operands::Collect => out.operands.push(arg.clone()),
                    Operands::Reject => return Err(self.unknown(arg, key)),
                    Operands::Max(max, usage) => {
                        if out.operands.len() == max {
                            return Err(CtlError::usage(usage));
                        }
                        out.operands.push(arg.clone());
                    }
                }
                continue;
            }
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
            if flag.kind == Kind::Greedy && inline.is_none() {
                while let Some(next) = iter.next_if(|next| !next.starts_with("--")) {
                    out.values.push((flag.name, next.clone()));
                }
                if !out.values.iter().any(|(k, _)| *k == flag.name) {
                    return Err(CtlError::usage(format!(
                        "{} requires {}",
                        flag.name, flag.needs
                    )));
                }
                continue;
            }
            let value = match inline {
                Some("") if flag.nonempty => {
                    return Err(CtlError::usage(format!(
                        "{} requires {}",
                        flag.name, flag.needs
                    )))
                }
                Some(value) => value.to_string(),
                None => iter.next().cloned().ok_or_else(|| {
                    CtlError::usage(format!("{} requires {}", flag.name, flag.needs))
                })?,
            };
            if let Some(bad) = flag.count {
                if value.parse::<u64>().is_err() {
                    return Err(CtlError::usage(format!("{} {bad}", flag.name)));
                }
            }
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

    pub(super) fn count(&self, name: &str) -> Option<u64> {
        self.one(name).and_then(|v| v.parse().ok())
    }

    pub(super) fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, CtlError> {
        self.one(name)
            .map(|v| {
                v.parse::<T>()
                    .map_err(|_| CtlError::usage(format!("{name} needs a whole number")))
            })
            .transpose()
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

    pub(super) fn entries(&self) -> impl Iterator<Item = (&'static str, Option<&str>)> + '_ {
        let switches = self.switches.iter().map(|name| (*name, None));
        let values = self.values.iter().map(|(name, v)| (*name, Some(v.as_str())));
        switches.chain(values)
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
