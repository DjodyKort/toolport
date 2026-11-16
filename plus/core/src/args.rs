use serde_json::Value;

pub(crate) fn flag_or(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

pub(crate) fn flag(args: &Value, key: &str) -> bool {
    flag_or(args, key, false)
}
