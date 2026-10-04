//! The fork keeps its own state in `unknown_fields` of the registry and of server entries
//! (`plus.selfmcp`, `plus.authProbe`, `mcpmSource`): upstream round-trips what it does not know.
//! These helpers read, create and prune such objects by key path.

use serde_json::{Map, Value};

pub(crate) type Fields = Map<String, Value>;

/// The object at `path`, when every step exists and is an object.
pub(crate) fn object<'a>(fields: &'a Fields, path: &[&str]) -> Option<&'a Fields> {
    let (first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(fields.get(*first)?, |value, key| value.get(*key))?
        .as_object()
}

/// The object at `path`, created on the way; anything that is not an object there is replaced.
pub(crate) fn object_mut<'a>(fields: &'a mut Fields, path: &[&str]) -> &'a mut Fields {
    let Some((first, rest)) = path.split_first() else {
        return fields;
    };
    let slot = fields
        .entry(first.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    object_mut(slot.as_object_mut().expect("just made an object"), rest)
}

/// Removes the entry at `path` and every parent object that it leaves empty.
pub(crate) fn remove(fields: &mut Fields, path: &[&str]) {
    match path {
        [] => {}
        [last] => {
            fields.remove(*last);
        }
        [first, rest @ ..] => {
            let Some(child) = fields.get_mut(*first).and_then(Value::as_object_mut) else {
                return;
            };
            remove(child, rest);
            if child.is_empty() {
                fields.remove(*first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(value: Value) -> Fields {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn object_follows_the_path_and_stops_at_non_objects() {
        let f = fields(json!({"plus": {"authProbe": {"kind": "slack"}, "n": 1}, "s": "x"}));
        assert_eq!(
            object(&f, &["plus", "authProbe"]).unwrap()["kind"],
            json!("slack")
        );
        assert!(object(&f, &["plus", "n"]).is_none());
        assert!(object(&f, &["plus", "missing"]).is_none());
        assert!(object(&f, &["s", "kind"]).is_none());
        assert!(object(&f, &[]).is_none());
    }

    #[test]
    fn object_mut_creates_the_path_and_replaces_scalars() {
        let mut f = fields(json!({"plus": "scalar", "keep": 1}));
        object_mut(&mut f, &["plus", "selfmcp"]).insert("optOut".into(), json!(true));
        assert_eq!(
            Value::Object(f.clone()),
            json!({"plus": {"selfmcp": {"optOut": true}}, "keep": 1})
        );
        object_mut(&mut f, &["plus", "selfmcp"]).insert("again".into(), json!(1));
        assert_eq!(f["plus"]["selfmcp"], json!({"optOut": true, "again": 1}));
        object_mut(&mut f, &[]).insert("top".into(), json!(2));
        assert_eq!(f["top"], json!(2));
    }

    #[test]
    fn remove_prunes_parents_it_empties_and_nothing_else() {
        let mut f = fields(json!({"plus": {"selfmcp": {"a": 1}}, "other": 1}));
        remove(&mut f, &["plus", "selfmcp"]);
        assert_eq!(Value::Object(f.clone()), json!({"other": 1}));

        let mut f = fields(json!({"plus": {"selfmcp": {"a": 1}, "authProbe": {}}}));
        remove(&mut f, &["plus", "selfmcp"]);
        assert_eq!(Value::Object(f.clone()), json!({"plus": {"authProbe": {}}}));

        let mut f = fields(json!({"plus": "scalar"}));
        remove(&mut f, &["plus", "selfmcp"]);
        remove(&mut f, &["absent", "x"]);
        assert_eq!(Value::Object(f), json!({"plus": "scalar"}));
    }
}
