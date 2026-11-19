use toolport_plus_api::MemoryHost;

use super::{read_resource, TOOLS};

fn tool_names_in(text: &str) -> Vec<&str> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|word| {
            word.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
        .filter(|word| word.contains('_') || ["doctor", "status"].contains(word))
        .collect()
}

#[test]
fn test01_every_tool_the_reference_documents_name_exists() {
    for uri in ["mcpm://architecture", "mcpm://workflows"] {
        let (text, mime) = read_resource(&MemoryHost::default(), uri).unwrap();
        assert_eq!(mime, "text/markdown");
        for word in tool_names_in(&text) {
            let known = TOOLS.iter().any(|tool| tool.name == word)
                || ["dry_run", "plan_hash", "dry_run=false"].contains(&word);
            assert!(known, "{uri} names `{word}`, which is not a tool");
        }
    }
}

#[test]
fn test02_the_inventories_are_empty_text_before_a_library_exists() {
    let host = MemoryHost::default();
    for uri in [
        "mcpm://inventory/skills",
        "mcpm://inventory/agents",
        "mcpm://inventory/styles",
    ] {
        let (text, mime) = read_resource(&host, uri).unwrap();
        assert_eq!((text.as_str(), mime), ("", "text/plain"), "{uri}");
    }
}
