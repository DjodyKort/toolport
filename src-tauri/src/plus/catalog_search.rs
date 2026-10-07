use crate::catalog;
use crate::plus::args::str_arg;
use serde_json::Value;

pub fn search_instant_handler(args: Value) -> Result<Value, String> {
    let query = str_arg(&args, "query").ok_or("missing argument: query")?;
    serde_json::to_value(catalog::search_instant(query)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use crate::plus::dispatch;
    use crate::plus::testutil::DataDirFx;
    use serde_json::json;

    #[test]
    fn instant_search_answers_curated_matches_without_a_registry_error() {
        let _fx = DataDirFx::new("toolport-catalog-search", "handler");
        let value = dispatch("plus.catalog.searchInstant", json!({"query": "slack"})).unwrap();
        let names: Vec<&str> = value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["name"].as_str())
            .collect();
        assert!(names.contains(&"Slack"), "{names:?}");
        assert!(value.get("registryError").is_none());
    }

    #[test]
    fn instant_search_requires_a_query() {
        let error = dispatch("plus.catalog.searchInstant", json!({})).unwrap_err();
        assert!(error.contains("query"), "{error}");
    }
}
