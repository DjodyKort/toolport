import { describe, expect, it } from "vitest";
import {
  draftsOf,
  kindOf,
  listingLength,
  patchOf,
  stillAsWritten,
  undoPatchOf,
} from "./frontmatterModel";

const FIELDS = {
  name: "nightly",
  description: "Runs the nightly checks",
  "allowed-tools": "Bash(git *)",
  priority: 3,
  "disable-model-invocation": false,
  hooks: { Stop: { command: "x.sh" } },
};

describe("frontmatterModel", () => {
  it("test01_text_yes_no_and_numbers_are_editable_and_the_rest_is_left_to_the_editor", () => {
    expect(draftsOf(FIELDS).map((d) => [d.key, d.kind])).toEqual([
      ["name", "locked"],
      ["description", "text"],
      ["allowed-tools", "text"],
      ["priority", "number"],
      ["disable-model-invocation", "boolean"],
      ["hooks", "complex"],
    ]);
    expect(kindOf("name", "x")).toBe("locked");
  });

  it("test02_the_patch_holds_only_what_changed_with_null_for_a_removed_field", () => {
    const drafts = draftsOf(FIELDS);
    drafts[1].text = "Runs the checks";
    drafts[2].removed = true;
    drafts[3].text = "3";
    drafts[4].text = "true";
    expect(patchOf(FIELDS, drafts)).toEqual({
      patch: {
        description: "Runs the checks",
        "allowed-tools": null,
        "disable-model-invocation": true,
      },
      problems: [],
    });
  });

  it("test03_a_number_that_is_not_one_is_a_problem_and_not_in_the_patch", () => {
    const drafts = draftsOf(FIELDS);
    drafts[3].text = "three";
    expect(patchOf(FIELDS, drafts)).toEqual({
      patch: {},
      problems: ["priority must be a number"],
    });
  });

  it("test04_an_added_field_is_sent_once_and_an_added_then_removed_one_not_at_all", () => {
    const drafts = [
      ...draftsOf(FIELDS),
      {
        key: "when_to_use",
        kind: "text" as const,
        text: "At night",
        removed: false,
        added: true,
      },
      { key: "license", kind: "text" as const, text: "MIT", removed: true, added: true },
    ];
    expect(patchOf(FIELDS, drafts).patch).toEqual({ when_to_use: "At night" });
  });

  it("test05_the_undo_patch_restores_old_values_and_removes_what_the_edit_added", () => {
    const patch = { description: "New", "allowed-tools": null, when_to_use: "x" };
    expect(undoPatchOf(FIELDS, patch)).toEqual({
      description: "Runs the nightly checks",
      "allowed-tools": "Bash(git *)",
      when_to_use: null,
    });
  });

  it("test06_the_fields_still_match_the_patch_only_when_nothing_changed_since", () => {
    const patch = { description: "New", priority: null };
    const written = { ...FIELDS, description: "New" } as Record<string, unknown>;
    delete written.priority;
    expect(stillAsWritten(written, patch)).toBe(true);
    expect(stillAsWritten({ ...written, description: "Other" }, patch)).toBe(false);
    expect(stillAsWritten({ ...written, priority: 3 }, patch)).toBe(false);
    expect(stillAsWritten({ allowed_tools: "x" }, { "allowed-tools": "x" })).toBe(true);
  });

  it("test07_the_listing_counts_description_and_when_to_use_and_skips_a_removed_one", () => {
    const drafts = draftsOf({ ...FIELDS, when_to_use: "At night" });
    expect(listingLength(drafts)).toEqual({ description: 23, whenToUse: 8 });
    drafts[drafts.length - 1].removed = true;
    expect(listingLength(drafts).whenToUse).toBe(0);
  });
});
