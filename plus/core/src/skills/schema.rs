//! Pydantic-style lax coercions shared by the agent and style frontmatter models.

use super::parser::{valid_name, want_str};
use serde_yaml::{Mapping, Value};

pub(crate) type Fm = [(String, Value)];

pub(crate) fn get<'a>(fm: &'a Fm, key: &str) -> Option<&'a Value> {
    fm.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// `Optional[str]`: absent or null is `None`, any non-string is a validation error.
pub(crate) fn opt_string(fm: &Fm, key: &str) -> Result<Option<String>, String> {
    match get(fm, key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => want_str(v, key).map(Some),
    }
}

/// `List[str] = []`: null is rejected, as pydantic does for a non-optional list.
pub(crate) fn string_list(fm: &Fm, key: &str) -> Result<Vec<String>, String> {
    match get(fm, key) {
        None => Ok(Vec::new()),
        Some(Value::Sequence(items)) => items.iter().map(|i| want_str(i, key)).collect(),
        Some(_) => Err(format!("{key}: input should be a valid list")),
    }
}

/// `bool` in lax mode: bools, 0/1 and the usual truthy/falsy strings.
pub(crate) fn lax_bool(fm: &Fm, key: &str, default: bool) -> Result<bool, String> {
    let err = || format!("{key}: input should be a valid boolean");
    match get(fm, key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(err()),
        },
        Some(Value::String(s)) => match s.trim().to_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "on" | "1" => Ok(true),
            "false" | "f" | "no" | "n" | "off" | "0" => Ok(false),
            _ => Err(err()),
        },
        Some(_) => Err(err()),
    }
}

/// `Optional[int]` in lax mode: integers, integral floats and numeric strings.
pub(crate) fn lax_opt_int(fm: &Fm, key: &str) -> Result<Option<i64>, String> {
    let err = || format!("{key}: input should be a valid integer");
    match get(fm, key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(i64::from(*b))),
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .map(Some)
            .ok_or_else(err),
        Some(Value::String(s)) => s.trim().parse::<i64>().map(Some).map_err(|_| err()),
        Some(_) => Err(err()),
    }
}

pub(crate) fn metadata(fm: &Fm) -> Result<Mapping, String> {
    match get(fm, "metadata") {
        None => Ok(Mapping::new()),
        Some(Value::Mapping(m)) => Ok(m.clone()),
        Some(_) => Err("metadata: input should be a valid dictionary".into()),
    }
}

/// Shared `name` / `description` validators of the skill, agent and style models.
pub(crate) fn name_and_description(fm: &Fm) -> Result<(String, String), String> {
    let name = want_str(get(fm, "name").ok_or("name: field required")?, "name")?;
    valid_name(&name)?;
    let description = want_str(
        get(fm, "description").ok_or("description: field required")?,
        "description",
    )?;
    let len = description.chars().count();
    if len == 0 || len > 1024 {
        return Err("description must be 1-1024 characters".into());
    }
    Ok((name, description))
}

pub(crate) fn version_of(metadata: &Mapping) -> Option<String> {
    metadata
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string)
}
