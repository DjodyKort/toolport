//! The JSON and the one-line-per-source text of a scan, shared by `sources ls` and the self-MCP
//! tool so both say the same thing.

use super::ScanReport;
use serde_json::{json, Value};

impl ScanReport {
    pub fn to_json(&self, with_items: bool) -> Value {
        let mut data = json!({
            "generatedAt": self.generated_at,
            "partial": self.partial,
            "skipped": self.skipped,
            "sources": self.sources,
        });
        if with_items {
            data["items"] = json!(self.items);
        }
        data
    }

    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for s in &self.sources {
            let c = &s.counts;
            text.push_str(&format!(
                "{:<34} {:<9} {}s {}c {}a {}r {}m  ~{} tokens  {}\n",
                s.id,
                s.status.state,
                c.skill,
                c.command,
                c.agent,
                c.rule,
                c.memory,
                s.tokens.value,
                s.status.detail
            ));
            if let Some(f) = &s.freshness {
                if f.behind > 0 || f.ahead > 0 || !f.in_checkout {
                    text.push_str(&format!(
                        "    {}: {} behind, {} ahead{}\n",
                        f.reference,
                        f.behind,
                        f.ahead,
                        if f.in_checkout {
                            ""
                        } else {
                            ", not all in the checkout"
                        }
                    ));
                }
            }
            for warning in &s.warnings {
                text.push_str(&format!("    warning: {warning}\n"));
            }
        }
        for skipped in &self.skipped {
            text.push_str(&format!(
                "partial: {} stopped: {}\n",
                skipped.detector, skipped.reason
            ));
        }
        if text.is_empty() {
            text.push_str("no sources found\n");
        }
        text
    }
}
