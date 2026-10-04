import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { skillsSyncData } from "../bridge/data";
import { addData, cleanData, resolveData, syncData, uninstallData } from "./fixtures";
import { installData, initData, unbundleData } from "./fixturesTaps";
import { installSpec, scopeArgs, specProblem } from "./model";
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

describe("skills plans: taps, install, bundles, init", () => {
  it("words the golden tap previews", () => {
    const add = planOf("skills tap add", goldenData("skills-tap-add.preview"), false)!;
    expect(add.summary).toMatch(/^Add the tap 'local' from /);
    expect(add.steps[0]).toMatchObject({ op: "create", path: "<WORLD>/data/taps/local" });
    expect(add.undo).toBe("toolportctl skills tap remove local");
    const remove = planOf(
      "skills tap remove",
      goldenData("skills-tap-remove.preview"),
      false,
    )!;
    expect(remove.steps).toEqual([
      { op: "delete", path: "<WORLD>/data/taps/local", detail: "Delete the local clone" },
    ]);
    expect(
      planOf("skills tap update", goldenData("skills-tap-update.preview"), false)!
        .summary,
    ).toBe("Update 1 tap");
    expect(
      planOf("skills tap update", goldenData("skills-tap-update.none"), false)!.summary,
    ).toBe("No taps to update");
  });

  it("words the golden install preview and a blocked one", () => {
    const plan = planOf("skills install", goldenData("skills-install.preview"), false)!;
    expect(plan.summary).toBe("Install 0 skills from acme-skills");
    expect(plan.steps[0].detail).toMatch(/^Clone and register the tap acme-skills/);
    const blocked = planOf(
      "skills install",
      installData("@acme/risky", { dryRun: true, blocked: true }),
      false,
    )!;
    expect(blocked.summary).toBe("Blocked: 1 high-severity audit finding in @acme/risky");
    expect(blocked.steps).toEqual([]);
    expect(blocked.warnings[0]).toMatch(/^high: deploy-risky: .* \(line 7\)$/);
    const skipped = planOf(
      "skills install",
      installData("@acme/risky", { dryRun: true, audit: false }),
      false,
    )!;
    expect(skipped.warnings[0]).toMatch(/audit was skipped/);
  });

  it("words bundle, unbundle (with what it overwrites) and init", () => {
    const bundle = planOf("skills bundle", goldenData("skills-bundle.preview"), false)!;
    expect(bundle.summary).toBe("Pack 1 skill (1 file) into a zip");
    expect(bundle.steps[0].detail).toBe("65 bytes of sources");
    expect(
      planOf("skills bundle", goldenData("skills-bundle.apply"), true)!.steps[0].detail,
    ).toBe("65 bytes of sources, 0 bytes zipped");
    const golden = planOf(
      "skills unbundle",
      goldenData("skills-unbundle.preview"),
      false,
    )!;
    expect(golden.steps).toEqual([
      { op: "create", path: "<WORLD>/fresh/skills/demo/SKILL.md", detail: "New file" },
    ]);
    const over = planOf("skills unbundle", unbundleData(true), false)!;
    expect(over.steps.map((s) => s.op)).toEqual(["create", "update"]);
    expect(over.warnings).toEqual(["1 file will be overwritten"]);
    const init = planOf("skills init", goldenData("skills-init.preview"), false)!;
    expect(init.summary).toBe("Create the skills repository 'contract' at <WORLD>/fresh");
    expect(init.steps).toHaveLength(6);
    expect(
      planOf("skills init", { ...initData(true), alreadyExists: true }, false)!.steps[0]
        .op,
    ).toBe("note");
  });

  it("derives the install spec of a hit only when it is a GitHub repository", () => {
    expect(installSpec({ repo: "acme/skills", name: "x" })).toBe("@acme/skills/x");
    expect(installSpec({ repo: "https://github.com/acme/skills.git", name: "x" })).toBe(
      "@acme/skills/x",
    );
    expect(installSpec({ repo: "/fixture/tap-src", name: "x" })).toBeNull();
    expect(specProblem("@acme/skills/x@1.2")).toBeNull();
    expect(specProblem("acme")).not.toBeNull();
    expect(scopeArgs({ project: true, dir: " /p " })).toEqual([
      "--project",
      "--repo",
      "/p",
    ]);
    expect(scopeArgs({ project: false, dir: "/p" })).toEqual([]);
  });
});
