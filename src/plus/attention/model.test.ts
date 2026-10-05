import { describe, expect, it } from "vitest";
import commands from "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json";
import type { CommandRow } from "../bridge/data";
import {
  ageText,
  actionArgs,
  commandOf,
  dismissArgv,
  groupItems,
  untilDate,
  viewOfRoute,
} from "./model";
import type { AttentionItem } from "../types/attention";

const rows = (commands as { envelope: { data: { commands: CommandRow[] } } }).envelope
  .data.commands;

const item = (id: string, level: AttentionItem["level"]): AttentionItem => ({
  id,
  level,
  title: id,
  detail: "",
  from: "doctor",
  target: { route: "servers", params: {} },
  action: null,
  since: "2026-10-04T08:00:00Z",
});

describe("attention model", () => {
  it("resolves each action argv the backend emits to its registry row and policy", () => {
    const resolve = (...words: string[]) => {
      const row = commandOf(rows, words);
      return row && { id: row.id, tier: row.tier, preview: row.preview?.mode };
    };
    expect(resolve("task", "resume", "run-1")).toEqual({
      id: "task resume",
      tier: "write",
      preview: "none",
    });
    expect(resolve("task", "run", "refresh")).toMatchObject({
      id: "task run",
      preview: "flag",
    });
    expect(resolve("auth", "probe", "--server", "alpha", "--force")).toMatchObject({
      id: "auth probe",
      tier: "read",
    });
    expect(resolve("compression", "presets", "--refresh")).toMatchObject({
      id: "compression presets",
      preview: "flag",
    });
    expect(
      resolve("context", "bundle", "apply", "web", "--cwd", "/home/demo/repo"),
    ).toMatchObject({ id: "context bundle apply", preview: "flag" });
  });

  it("finds no row for an unknown command and none before the registry loads", () => {
    expect(commandOf(rows, ["frobnicate", "now"])).toBeNull();
    expect(commandOf(null, ["task", "resume", "run-1"])).toBeNull();
  });

  it("only runs an action whose argv starts with toolportctl", () => {
    expect(
      actionArgs({ label: "Go", command: ["toolportctl", "task", "run", "a"] }),
    ).toEqual(["task", "run", "a"]);
    expect(actionArgs({ label: "Go", command: ["sh", "-c", "task run a"] })).toBeNull();
    expect(actionArgs({ label: "Go", command: ["toolportctl"] })).toBeNull();
    expect(actionArgs({ label: "Go", command: [] })).toBeNull();
  });

  it("puts each row in its level group and keeps the order", () => {
    const groups = groupItems([
      item("a", "look"),
      item("b", "needs-you"),
      item("c", "look"),
    ]);
    expect(groups["needs-you"].map((i) => i.id)).toEqual(["b"]);
    expect(groups.look.map((i) => i.id)).toEqual(["a", "c"]);
    expect(groups.fyi).toEqual([]);
  });

  it("maps a route to a view, and gives an unknown route no view", () => {
    expect(viewOfRoute("servers")).toBe("control");
    expect(viewOfRoute("library")).toBe("library");
    expect(viewOfRoute("tasks")).toBe("tasks");
    expect(viewOfRoute("nowhere")).toBeNull();
  });

  it("dismisses until a local calendar date, or for good without one", () => {
    const now = new Date(2026, 11, 30, 23, 30);
    expect(untilDate("tomorrow", now)).toBe("2026-12-31");
    expect(untilDate("week", now)).toBe("2027-01-06");
    expect(untilDate("forever", now)).toBeNull();
    expect(dismissArgv("auth:alpha", "2027-01-06")).toEqual([
      "attention",
      "dismiss",
      "auth:alpha",
      "--until",
      "2027-01-06",
    ]);
    expect(dismissArgv("auth:alpha", null)).toEqual([
      "attention",
      "dismiss",
      "auth:alpha",
    ]);
  });

  it("words the age of a row", () => {
    const now = Date.parse("2026-10-05T12:00:00Z");
    expect(ageText("2026-10-05T11:59:40Z", now)).toBe("just now");
    expect(ageText("2026-10-05T11:30:00Z", now)).toBe("30 min");
    expect(ageText("2026-10-05T09:00:00Z", now)).toBe("3 h");
    expect(ageText("2026-10-02T12:00:00Z", now)).toBe("3 d");
    expect(ageText("garbage", now)).toBe("");
  });
});
