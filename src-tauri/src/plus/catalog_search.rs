use crate::catalog;
use crate::catalog_index::{self, SyncMode};
use crate::plus::args::{flag, str_arg};
use serde_json::{json, Value};

pub fn search_instant_handler(args: Value) -> Result<Value, String> {
    let query = str_arg(&args, "query").ok_or("missing argument: query")?;
    serde_json::to_value(catalog::search_instant(query)).map_err(|e| e.to_string())
}

pub fn index_status_handler(_args: Value) -> Result<Value, String> {
    Ok(catalog_index::status())
}

pub fn index_sync_handler(args: Value) -> Result<Value, String> {
    let mode = if flag(&args, "full") {
        SyncMode::Full
    } else {
        SyncMode::Incremental
    };
    catalog_index::sync(mode, &mut |_| {})
        .map(|report| json!(report))
        .map_err(|error| error.message())
}

pub fn index_tick_handler(args: Value) -> Result<Value, String> {
    catalog_index::tick(flag(&args, "force"))
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
    fn index_status_reads_the_local_index_without_the_network() {
        let _fx = DataDirFx::new("toolport-catalog-search", "index-status");
        let value = dispatch("plus.catalog.indexStatus", json!({})).unwrap();
        assert_eq!(value["count"], 0);
        assert_eq!(value["complete"], false);
        assert_eq!(value["running"], false);
        assert!(value["lastSyncedAt"].is_null());
    }

    #[test]
    fn instant_search_reports_that_a_live_call_is_needed_without_an_index() {
        let _fx = DataDirFx::new("toolport-catalog-search", "live-needed");
        let value = dispatch("plus.catalog.searchInstant", json!({"query": "slack"})).unwrap();
        assert_eq!(value["liveNeeded"], true);
    }

    #[test]
    fn instant_search_requires_a_query() {
        let error = dispatch("plus.catalog.searchInstant", json!({})).unwrap_err();
        assert!(error.contains("query"), "{error}");
    }
}
