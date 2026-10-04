import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { contextShapes } from "../types/context";
import { contextPlan, stepOf } from "./plans";
import { SHIMS, checkpointData, deployData, emptyStatus, statusData } from "./fixtures";
import { goldenData } from "./testkit";

describe("context fixtures", () => {
  it("have exactly the shape of the golden envelopes", () => {
    expect(check(contextShapes["context-status"], statusData)).toEqual([]);
    expect(check(contextShapes["context-status"], emptyStatus)).toEqual([]);
    expect(check(contextShapes["context-plan"], deployData(true, false))).toEqual([]);
    expect(check(contextShapes["context-apply.apply"], deployData(false, false))).toEqual(
      [],
    );
    expect(
      check(contextShapes["context-checkpoint-status.status"], checkpointData),
    ).toEqual([]);
  });
});

describe("stepOf", () => {
  it.each([
    [`would write shims: ${SHIMS}`, "update", SHIMS],
    [`wrote shims: ${SHIMS}`, "update", SHIMS],
    ["would remove shims file", "delete", undefined],
    ["would remove profile dir /p/work", "delete", "/p/work"],
    ["removed profile dir /p/work", "delete", "/p/work"],
    ["would generate launch profile work (0 server(s)) in /p/work", "create", "/p/work"],
    ["would back up /h/.zshrc as /h/.zshrc.bak", "create", "/h/.zshrc.bak"],
    ["would rewrite 3 line(s) of /h/.zshrc", "update", "/h/.zshrc"],
    ["saved config (0 profile(s))", "update", undefined],
    ["something new the CLI says", "note", undefined],
  ])("words %j as %s", (line, op, path) => {
    const step = stepOf(line);
    expect(step.op).toBe(op);
    expect(step.path).toBe(path);
  });
});

describe("contextPlan", () => {
  it("lists for the preview of every golden command the files its actions name", () => {
    for (const [command, stem, count] of [
      ["context apply", "context-apply.preview", 1],
      ["context disable", "context-disable.preview", 2],
      ["context profile add", "context-profile-add.preview", 2],
      ["context profile remove", "context-profile-remove.preview", 2],
    ] as const) {
      const plan = contextPlan(command, goldenData(stem), false)!;
      expect(plan.steps.filter((s) => s.op !== "note")).toHaveLength(count);
      expect(plan.steps.every((s) => s.op !== "note")).toBe(true);
    }
  });

  it("takes the plan of a sync preview and its apply once it is done", () => {
    const sync = goldenData("context-sync.apply");
    expect(contextPlan("context sync", sync, false)!.steps).toHaveLength(1);
    const done = contextPlan("context sync", sync, true)!;
    expect(done.summary).toBe("Deployed");
    expect(done.steps.map((s) => s.op)).toEqual(["update", "update"]);
  });

  it("turns the checks that are not ok into warnings and the zshrc changes into diffs", () => {
    const plan = contextPlan("context apply", deployData(true, true), false)!;
    expect(plan.warnings.join("\n")).toMatch(/sourced BEFORE shell-wrapper/);
    expect(plan.warnings.join("\n")).toMatch(/sourced before shell-wrapper/);
    const diff = plan.steps.find((s) => s.diff);
    expect(diff?.diff?.before).toBe("source ~/.config/mcpm/context-shims.zsh");
  });

  it("scaffolds the personal layer and a client layer as created files", () => {
    const init = contextPlan("context init", goldenData("context-init.preview"), false)!;
    expect(init.steps.map((s) => s.op)).toEqual(["create", "update"]);
    const client = contextPlan(
      "context client add",
      goldenData("context-client-add.preview"),
      false,
    )!;
    expect(client.steps).toEqual([
      expect.objectContaining({
        op: "create",
        path: expect.stringContaining("client-acme"),
      }),
    ]);
  });
});
