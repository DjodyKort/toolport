//! Security audit for skill bodies. The pattern table is copied verbatim from mcpm, including
//! the two upper-case patterns that can never match because lines are lower-cased first.

use super::parser::Skill;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditFinding {
    pub severity: &'static str,
    pub skill_name: String,
    pub message: String,
    pub line: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditResult {
    pub findings: Vec<AuditFinding>,
}

impl AuditResult {
    pub fn has_high_severity(&self) -> bool {
        self.findings.iter().any(|f| f.severity == "high")
    }
}

const PATTERNS: &[(&str, &str, &str)] = &[
    (
        r"ignore\s+(all\s+)?previous\s+(instructions?|context|rules)",
        "high",
        "Prompt injection: ignore previous instructions",
    ),
    (
        r"disregard\s+(all\s+)?prior\s+(instructions?|context)",
        "high",
        "Prompt injection: disregard prior context",
    ),
    (
        r"you\s+are\s+now\s+(a|an)\s+",
        "medium",
        "Possible prompt injection: role override attempt",
    ),
    (
        r"override\s+(safety|security|system)\s+(guidelines?|rules?|restrictions?)",
        "high",
        "Prompt injection: safety override attempt",
    ),
    (
        r"system\s*:\s*you\s+are",
        "high",
        "Prompt injection: fake system prompt",
    ),
    (
        r"curl\s+.*\|\s*bash",
        "high",
        "Data exfiltration risk: piping curl to bash",
    ),
    (
        r"wget\s+.*-O\s*-\s*\|\s*sh",
        "high",
        "Data exfiltration risk: piping wget to shell",
    ),
    (
        r"base64\s+.*\|\s*curl",
        "high",
        "Data exfiltration risk: base64 encoding piped to curl",
    ),
    (
        r"curl\s+-[dX]\s+POST\s+.*\$\(cat",
        "high",
        "Data exfiltration risk: posting file contents via curl",
    ),
    (
        r"eval\s*\(\s*fetch\s*\(",
        "medium",
        "Suspicious: eval of fetched content",
    ),
    (
        r"rm\s+-rf\s+[/~]",
        "high",
        "Dangerous command: recursive delete of system paths",
    ),
    (
        r"chmod\s+777",
        "medium",
        "Suspicious: setting world-writable permissions",
    ),
    (
        r"sudo\s+",
        "medium",
        "Suspicious: sudo usage in skill instructions",
    ),
    (
        r">\s*/etc/",
        "high",
        "Dangerous: writing to system config directory",
    ),
];

fn compiled() -> &'static Vec<(Regex, &'static str, &'static str)> {
    static RE: OnceLock<Vec<(Regex, &'static str, &'static str)>> = OnceLock::new();
    RE.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|(p, sev, msg)| (Regex::new(p).expect("audit pattern"), *sev, *msg))
            .collect()
    })
}

pub fn audit_skill(skill: &Skill) -> AuditResult {
    let mut result = AuditResult::default();
    for (idx, line) in skill.body.split('\n').enumerate() {
        let lowered = line.to_lowercase();
        for (re, severity, message) in compiled() {
            if re.is_match(&lowered) {
                result.findings.push(AuditFinding {
                    severity,
                    skill_name: skill.name().to_string(),
                    message: (*message).to_string(),
                    line: idx + 1,
                });
            }
        }
    }
    if let Some(tools) = skill
        .frontmatter
        .allowed_tools
        .as_deref()
        .filter(|t| !t.is_empty())
    {
        let broad: Vec<&str> = tools
            .split_whitespace()
            .filter(|t| ["Bash", "Bash(*)", "Write", "Edit"].contains(t))
            .collect();
        if broad.len() >= 3 {
            result.findings.push(AuditFinding {
                severity: "medium",
                skill_name: skill.name().to_string(),
                message: format!(
                    "Broad tool permissions: {}. Consider restricting to specific commands.",
                    broad.join(", ")
                ),
                line: 0,
            });
        }
    }
    result
}

pub fn audit_skills(skills: &[Skill]) -> AuditResult {
    let mut result = AuditResult::default();
    for skill in skills {
        result.findings.extend(audit_skill(skill).findings);
    }
    result
}
