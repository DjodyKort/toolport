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
            kind,
            needs: "a value",
        }
    }

    pub(super) const fn needs(self, what: &'static str) -> Self {
        Self { needs: what, ..self }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub flags: &'static [Flag],
}

#[derive(Default)]
pub(super) struct Flags {
    values: Vec<(&'static str, String)>,
    switches: Vec<&'static str>,
    operands: Vec<String>,
}

impl Spec {
    pub(super) const PLAIN: Spec = Spec { flags: &[] };

    pub(super) fn parse(&self, rest: &[String]) -> Result<Flags, CtlError> {
        let mut out = Flags::default();
        let mut iter = rest.iter();
        while let Some(arg) = iter.next() {
            if !arg.starts_with('-') || arg == "-" {
                out.operands.push(arg.clone());
                continue;
            }
            let Some(flag) = self.flags.iter().find(|f| f.name == arg) else {
                return Err(CtlError::usage(format!("unknown option: {arg}")));
            };
            if flag.kind == Kind::Switch {
                out.switches.push(flag.name);
                continue;
            }
            let value = iter.next().ok_or_else(|| {
                CtlError::usage(format!("{} requires {}", flag.name, flag.needs))
            })?;
            out.values.push((flag.name, value.clone()));
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
