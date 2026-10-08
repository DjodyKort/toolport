import { describe, expect, it } from "vitest";
import { sourceLabel } from "./HooksTab";

describe("the source of a hook", () => {
  it("names the event and the position instead of the config path inside the file", () => {
    expect(sourceLabel("/root/.claude/settings.json#hooks.PreToolUse[0].hooks[1]")).toBe(
      "/root/.claude/settings.json, PreToolUse hook 1.2",
    );
  });

  it("leaves a source it does not recognise as it is", () => {
    expect(sourceLabel("/root/plugin/hooks.json")).toBe("/root/plugin/hooks.json");
  });
});
