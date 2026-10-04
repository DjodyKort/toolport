//! The plan and result shapes every writer of the GUI wave shares (contract section 0): a dry run
//! returns a `PlanV1`, an apply returns a `ResultV1`.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diff {
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Step {
    pub op: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Diff>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TokenEffect {
    pub before: u64,
    pub after: u64,
    pub basis: &'static str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Effects {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenEffect>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlanV1 {
    pub summary: String,
    pub steps: Vec<Step>,
    pub effects: Effects,
    pub warnings: Vec<String>,
    pub undo: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResultV1 {
    pub applied: bool,
    pub changed: Vec<String>,
    pub undo: String,
    pub backups: Vec<String>,
}

impl Step {
    pub fn merge(path: &str, detail: impl Into<String>, keys: &[&str], diff: Diff) -> Self {
        Self {
            op: "merge",
            path: Some(path.to_string()),
            detail: detail.into(),
            keys: Some(keys.iter().map(|k| k.to_string()).collect()),
            diff: Some(diff),
        }
    }

    pub fn create(path: &str, detail: impl Into<String>, keys: &[&str], diff: Diff) -> Self {
        Self {
            op: "create",
            ..Self::merge(path, detail, keys, diff)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_plan_serializes_to_the_contract_shape() {
        let plan = PlanV1 {
            summary: "s".into(),
            steps: vec![Step {
                op: "note",
                path: None,
                detail: "d".into(),
                keys: None,
                diff: None,
            }],
            effects: Effects::default(),
            warnings: vec![],
            undo: "u".into(),
        };
        assert_eq!(
            serde_json::to_value(plan).unwrap(),
            json!({"summary": "s", "steps": [{"op": "note", "detail": "d"}], "effects": {}, "warnings": [], "undo": "u"})
        );
    }
}
