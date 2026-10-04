import { describe, expect, it } from "vitest";
import { resolveShortcut } from "@/lib/shortcuts";
import {
  NUMBER_KEY_VIEWS,
  nextTabIndex,
  screenKeyAllowed,
  type ScreenKeyEvent,
} from "./keyboard";

const key = (k: string, over: Partial<ScreenKeyEvent> = {}): ScreenKeyEvent => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  ...over,
});

describe("keyboard policy of the Toolport+ screens", () => {
  it("leaves the number keys at the shipped seven views", () => {
    expect(NUMBER_KEY_VIEWS).toBe(7);
    const event = (k: string) => ({
      key: k,
      ctrlKey: true,
      metaKey: false,
      altKey: false,
      shiftKey: false,
    });
    expect(resolveShortcut(event("7"))).toMatchObject({ kind: "view" });
    expect(resolveShortcut(event("8"))).toBeNull();
    expect(resolveShortcut(event("9"))).toBeNull();
  });

  it("lets a screen use a bare key only outside fields and never the shell's keys", () => {
    expect(screenKeyAllowed(key("j"), { tagName: "DIV" })).toBe(true);
    expect(screenKeyAllowed(key("j"), { tagName: "INPUT" })).toBe(false);
    expect(screenKeyAllowed(key("j"), { tagName: "TEXTAREA" })).toBe(false);
    expect(screenKeyAllowed(key("j"), { isContentEditable: true })).toBe(false);
    for (const reserved of ["/", "?", "Escape"]) {
      expect(screenKeyAllowed(key(reserved), { tagName: "DIV" })).toBe(false);
    }
    expect(screenKeyAllowed(key("j", { ctrlKey: true }))).toBe(false);
    expect(screenKeyAllowed(key("j", { metaKey: true }))).toBe(false);
    expect(screenKeyAllowed(key("j", { altKey: true }))).toBe(false);
  });

  it("moves between tabs with the arrow keys, Home and End, wrapping at the ends", () => {
    expect(nextTabIndex("ArrowRight", 0, 3)).toBe(1);
    expect(nextTabIndex("ArrowRight", 2, 3)).toBe(0);
    expect(nextTabIndex("ArrowLeft", 0, 3)).toBe(2);
    expect(nextTabIndex("ArrowLeft", 2, 3)).toBe(1);
    expect(nextTabIndex("Home", 2, 3)).toBe(0);
    expect(nextTabIndex("End", 0, 3)).toBe(2);
    expect(nextTabIndex("ArrowDown", 0, 3)).toBeNull();
    expect(nextTabIndex("ArrowDown", 0, 3, "vertical")).toBe(1);
    expect(nextTabIndex("ArrowRight", 0, 3, "vertical")).toBeNull();
    expect(nextTabIndex("a", 0, 3)).toBeNull();
    expect(nextTabIndex("ArrowRight", 0, 0)).toBeNull();
  });
});
