use super::op::OpError;
use serde_json::Value;

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(crate) fn str_nonempty<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    str_arg(args, key).filter(|s| !s.is_empty())
}

pub(crate) fn flag_or(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub(crate) fn flag(args: &Value, key: &str) -> bool {
    flag_or(args, key, false)
}

pub(crate) fn list<'a>(args: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    args.get(key).and_then(Value::as_array)
}

pub(crate) fn nonempty_strings(args: &Value, key: &str) -> Option<Vec<String>> {
    let keys: Vec<String> = list(args, key)?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    (!keys.is_empty()).then_some(keys)
}

pub(crate) fn is_string_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

/// Rejects an argument object the way the self-MCP tool schemas do: not an object, a key that is
/// not listed, or a listed key whose value is neither null nor of the listed type.
pub(crate) fn check(args: &Value, keys: &[(&str, fn(&Value) -> bool)]) -> Result<(), OpError> {
    let Some(map) = args.as_object() else {
        return Err(OpError::usage("arguments must be an object"));
    };
    for key in map.keys() {
        if !keys.iter().any(|(name, _)| name == key) {
            return Err(OpError::usage(format!("unknown argument: {key}")));
        }
    }
    for (name, accepts) in keys {
        match map.get(*name) {
            Some(value) if !value.is_null() && !accepts(value) => {
                return Err(OpError::usage(format!(
                    "argument {name} has the wrong type"
                )))
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn getters_read_typed_keys_and_ignore_other_types() {
        let args = json!({"s": "x", "e": "", "b": true, "n": 3, "l": ["a", 1]});
        assert_eq!(str_arg(&args, "s"), Some("x"));
        assert_eq!(str_arg(&args, "e"), Some(""));
        assert_eq!(str_arg(&args, "n"), None);
        assert_eq!(str_nonempty(&args, "e"), None);
        assert_eq!(str_nonempty(&args, "s"), Some("x"));
        assert!(flag(&args, "b"));
        assert!(!flag(&args, "s"));
        assert!(flag_or(&args, "missing", true));
        assert!(!flag_or(&args, "missing", false));
        assert_eq!(list(&args, "l").map(Vec::len), Some(2));
        assert!(list(&args, "s").is_none());
    }

    #[test]
    fn nonempty_strings_keeps_the_strings_and_drops_an_empty_result() {
        let args = json!({"a": ["x", 1, "y"], "b": [], "c": [1], "d": "x"});
        assert_eq!(nonempty_strings(&args, "a"), Some(vec!["x".into(), "y".into()]));
        assert_eq!(nonempty_strings(&args, "b"), None);
        assert_eq!(nonempty_strings(&args, "c"), None);
        assert_eq!(nonempty_strings(&args, "d"), None);
        assert_eq!(nonempty_strings(&args, "missing"), None);
    }

    #[test]
    fn check_rejects_what_a_tool_schema_would() {
        let keys: &[(&str, fn(&Value) -> bool)] = &[
            ("path", Value::is_string),
            ("dry", Value::is_boolean),
            ("names", is_string_list),
        ];
        assert!(check(&json!({}), keys).is_ok());
        assert!(check(&json!({"path": "p", "dry": true, "names": ["a"]}), keys).is_ok());
        assert!(check(&json!({"path": null}), keys).is_ok());
        let message = |args: Value| check(&args, keys).unwrap_err().message;
        assert_eq!(message(json!([])), "arguments must be an object");
        assert_eq!(message(json!({"other": 1})), "unknown argument: other");
        assert_eq!(message(json!({"path": 1})), "argument path has the wrong type");
        assert_eq!(message(json!({"names": ["a", 1]})), "argument names has the wrong type");
    }
}
