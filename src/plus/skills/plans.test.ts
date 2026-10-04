import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { skillsSyncData } from "../bridge/data";
import { addData, cleanData, resolveData, syncData, uninstallData } from "./fixtures";
import { planOf } from "./plans";
import { goldenData } from "./testkit";

describe("skills plans", () => {
  it("words the real sync preview as a plan", () => {
    const data = goldenData("skills-sync.preview");
    expect(check(skillsSyncData, data)).toEqual([]);
    const plan = planOf("skills sync", data, false)!;
    expect(plan.summary).toBe("Write 1 skill and 0 rules to 1 client");
    expect(plan.steps).toEqual([
      { op: "update", path: "<WORLD>/home", detail: "Claude Code: 1 entry" },
    ]);
    expect(plan.undo).toBe("toolportctl skills clean");
    expect(planOf("skills sync", { ...data, dryRun: false }, true)!.summary).toMatch(
      /^Wrote /,
    );
  });

  it("carries a transpiler warning and a collision of a sync into the plan", () => {
    const plan = planOf("skills sync", syncData(["claude-code", "cursor"], true), false)!;
    expect(plan.warnings).toEqual([
      "deploy-helper: cursor: 'allowed-tools' field not supported, dropped",
    ]);
    expect(plan.steps.filter((s) => s.op === "note").map((s) => s.detail)).toEqual([
      "release-notes (Claude Code): the command file shadows the skill and stays",
    ]);
  });

  it("lists what clean, uninstall and resolve delete or move", () => {
    const clean = planOf("skills clean", goldenData("skills-clean.preview"), false)!;
    expect(clean.summary).toBe("Remove 1 synced skill file");
    expect(clean.steps.map((s) => s.op)).toEqual(["delete", "delete", "note"]);
    expect(planOf("skills clean", cleanData(true), false)!.undo).toBe(
      "toolportctl skills sync",
    );
    const gone = planOf(
      "skills uninstall",
      goldenData("skills-uninstall.preview"),
      false,
    )!;
    expect(gone.summary).toBe("Remove the skill 'demo' and its 1 output");
    expect(gone.steps.map((s) => s.op)).toEqual(["delete", "delete", "update"]);
    expect(planOf("skills uninstall", uninstallData("x", true), true)!.summary).toMatch(
      /^Removed /,
    );
    const resolve = planOf("skills resolve", resolveData(true, true), false)!;
    expect(resolve.summary).toBe("Resolve 1 collision");
    expect(resolve.steps[0]).toMatchObject({ op: "update" });
    expect(
      planOf("skills resolve", goldenData("skills-resolve.preview"), false)!.summary,
    ).toBe("Nothing shadows a synced skill");
  });

  it("names the file a new skill creates and how to undo it", () => {
    const plan = planOf("skills add", goldenData("skills-add.preview"), false)!;
    expect(plan.summary).toBe("Create the skill 'fresh-skill' from the template");
    expect(plan.steps).toEqual([
      {
        op: "create",
        path: "<WORLD>/fresh/skills/fresh-skill/SKILL.md",
        detail: "New skill file",
      },
    ]);
    expect(plan.undo).toBe("toolportctl skills uninstall fresh-skill");
    expect(planOf("skills add", addData("r", "rule", true), false)!.steps[0].detail).toBe(
      "New rule file",
    );
  });

  it("has no plan for a command it does not word", () => {
    expect(planOf("skills lint", {}, false)).toBeNull();
  });
});
