import { describe, expect, it } from "vitest";
import { lineDiff } from "./lineDiff";

describe("lineDiff", () => {
  it("test01_an_edited_line_is_one_removal_and_one_addition_with_context", () => {
    const diff = lineDiff("a\nb\nc\nd\n", "a\nB\nc\nd\n");
    expect(diff.lines).toEqual([
      { sign: " ", text: "a" },
      { sign: "-", text: "b" },
      { sign: "+", text: "B" },
      { sign: " ", text: "c" },
      { sign: " ", text: "d" },
    ]);
    expect([diff.added, diff.removed, diff.coarse]).toEqual([1, 1, false]);
  });

  it("test02_lines_far_from_a_change_are_left_out", () => {
    const body = Array.from({ length: 40 }, (_, n) => `line ${n}`);
    const after = [...body];
    after[20] = "changed";
    const diff = lineDiff(body.join("\n"), after.join("\n"));
    expect(diff.lines.length).toBe(3 + 2 + 3);
    expect(diff.lines.filter((l) => l.sign === " ").length).toBe(6);
  });

  it("test03_inserted_and_deleted_blocks_line_up_around_the_lines_they_share", () => {
    const diff = lineDiff(
      "one\ntwo\nthree\nfour\n",
      "one\nthree\nthree and a half\nfour\n",
    );
    expect(diff.lines.filter((l) => l.sign !== " ")).toEqual([
      { sign: "-", text: "two" },
      { sign: "+", text: "three and a half" },
    ]);
  });

  it("test04_text_that_differs_only_after_its_last_line_has_no_change", () => {
    const diff = lineDiff("a\nb\n", "a\nb\n\n\n");
    expect([diff.added, diff.removed]).toEqual([0, 0]);
  });

  it("test05_a_body_too_different_to_line_up_is_shown_as_removed_then_added", () => {
    const before = Array.from({ length: 2500 }, (_, n) => `old ${n}`).join("\n");
    const after = Array.from({ length: 2500 }, (_, n) => `new ${n}`).join("\n");
    const diff = lineDiff(before, after);
    expect(diff.coarse).toBe(true);
    expect([diff.removed, diff.added]).toEqual([2500, 2500]);
  });
});
