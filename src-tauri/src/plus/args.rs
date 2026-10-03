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
}
