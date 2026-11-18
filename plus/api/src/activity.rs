//! What the gateway's audit log says about when servers were used.

use std::collections::BTreeMap;

pub trait ServerActivity {
    /// Unix time in seconds of the latest successful tool call of each server in `server_ids`,
    /// by id; a server with no successful call in the retained log is absent. The log keeps only
    /// the most recent calls, so a missing server may have been used before it starts.
    fn last_successful_calls(&self, server_ids: &[String])
        -> Result<BTreeMap<String, i64>, String>;
}
