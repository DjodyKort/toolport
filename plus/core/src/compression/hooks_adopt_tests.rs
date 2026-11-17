use super::hooks::{adopt_edit, apply, read_ledger, undo, unwrap_original, wrap};
use super::hooks_testkit::{rtk_settings, world, World};
use serde_json::json;

fn wrapped_then_edited(edit: impl Fn(&str) -> String) -> World {
    let w = world(rtk_settings());
    apply(&w.host, &w.paths, None, false).unwrap();
    w.set_command(&edit(&w.wrapped_rtk()));
    assert_eq!(w.health("rtk"), "edited");
    w
}

#[test]
fn test01_adopting_a_suffixed_edit_makes_it_ok_and_undo_restores_the_edit() {
    let w = wrapped_then_edited(|c| format!("{c}  # kept"));
    let edited = w.command();
    let settings_before = std::fs::read(&w.file).unwrap();
    let data = adopt_edit(&w.host, &w.paths, "rtk", false).unwrap();
    assert_eq!(data["rows"][0]["action"], "adopted");
    assert_eq!(w.health("rtk"), "ok");
    assert_eq!(std::fs::read(&w.file).unwrap(), settings_before);
    undo(&w.host, &w.paths, None, false).unwrap();
    assert_eq!(w.command(), edited);
    assert_eq!(w.entries(), 0);
}

#[test]
fn test02_adopting_an_edit_inside_the_wrapper_makes_undo_restore_the_edited_command() {
    let w = wrapped_then_edited(|_| wrap("rtk", "rtk hook claude --fast"));
    adopt_edit(&w.host, &w.paths, "rtk", false).unwrap();
    assert_eq!(w.health("rtk"), "ok");
    undo(&w.host, &w.paths, None, false).unwrap();
    assert_eq!(w.command(), "rtk hook claude --fast");
}

#[test]
fn test03_an_entry_that_is_not_edited_cannot_be_adopted() {
    let w = world(rtk_settings());
    let foreign = adopt_edit(&w.host, &w.paths, "rtk", false).unwrap_err();
    assert!(foreign.contains("reads as edited"), "{foreign}");
    apply(&w.host, &w.paths, None, false).unwrap();
    assert!(adopt_edit(&w.host, &w.paths, "rtk", false).is_err());
    assert!(adopt_edit(&w.host, &w.paths, "nope", false).is_err());
}

#[test]
fn test04_revert_puts_the_ledger_back_and_restore_ours_works_again() {
    let w = wrapped_then_edited(|c| format!("{c}  # kept"));
    let ledger_before = read_ledger(&w.paths).unwrap().entries;
    adopt_edit(&w.host, &w.paths, "rtk", false).unwrap();
    let data = adopt_edit(&w.host, &w.paths, "rtk", true).unwrap();
    assert_eq!(data["rows"][0]["action"], "reverted");
    let ledger = read_ledger(&w.paths).unwrap();
    assert_eq!(ledger.entries, ledger_before);
    assert!(ledger.adopted.is_empty());
    assert_eq!(w.health("rtk"), "edited");
    let restored = apply(&w.host, &w.paths, None, false).unwrap();
    assert_eq!(restored["rows"][0]["action"], "restored ours");
    assert!(adopt_edit(&w.host, &w.paths, "rtk", true).is_err());
}

#[test]
fn test05_a_wrapped_entry_the_ledger_never_heard_of_is_adopted_and_a_revert_forgets_it() {
    let w = world(json!({"hooks": {"PreToolUse": [
        {"matcher": "Bash", "hooks": [{"type": "command", "command": wrap("rtk", "rtk hook claude")}]}
    ]}}));
    assert_eq!(w.health("rtk"), "edited");
    adopt_edit(&w.host, &w.paths, "rtk", false).unwrap();
    assert_eq!(w.health("rtk"), "ok");
    let events: Vec<_> = read_ledger(&w.paths)
        .unwrap()
        .epochs
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(events, ["wrap"]);
    adopt_edit(&w.host, &w.paths, "rtk", true).unwrap();
    assert_eq!(w.entries(), 0);
    assert_eq!(w.health("rtk"), "edited");
}

#[test]
fn test06_wrapping_again_forgets_an_adoption() {
    let w = wrapped_then_edited(|c| format!("{c}  # kept"));
    adopt_edit(&w.host, &w.paths, "rtk", false).unwrap();
    undo(&w.host, &w.paths, None, false).unwrap();
    assert!(read_ledger(&w.paths).unwrap().adopted.is_empty());
}

#[test]
fn test07_unwrap_only_reads_back_what_wrap_wrote() {
    for original in ["rtk hook claude", "env A='b c' rtk", "echo it's"] {
        assert_eq!(
            unwrap_original(&wrap("rtk", original), "rtk").as_deref(),
            Some(original)
        );
    }
    let wrapped = wrap("rtk", "rtk hook claude");
    assert_eq!(unwrap_original(&format!("{wrapped} # x"), "rtk"), None);
    assert_eq!(unwrap_original("rtk hook claude", "rtk"), None);
    assert_eq!(unwrap_original(&wrapped, "read-once"), None);
}
