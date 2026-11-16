use serde_json::Value;

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(crate) fn list<'a>(args: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    args.get(key).and_then(Value::as_array)
}

pub(crate) fn flag_or(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub(crate) fn flag(args: &Value, key: &str) -> bool {
    flag_or(args, key, false)
}

pub(crate) fn nonempty_strings(args: &Value, key: &str) -> Option<Vec<String>> {
    let keys: Vec<String> = list(args, key)?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    (!keys.is_empty()).then_some(keys)
}

pub(crate) fn str_nonempty<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    str_arg(args, key).filter(|s| !s.is_empty())
}
