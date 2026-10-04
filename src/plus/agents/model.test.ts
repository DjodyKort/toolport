import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { CommandRow } from "../bridge/data";
import { agentsCommandRows } from "../fixtures/agents";
import { droppedOf, nameProblem, policyOf, warnsClient, shortPath } from "./model";
import { planOf } from "./plans";

const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");
const golden = (stem: string) =>
  JSON.parse(readFileSync(join(dir, `${stem}.json`), "utf8")).envelope.data;
const rows: CommandRow[] = JSON.parse(readFileSync(join(dir, "commands.json"), "utf8"))
  .envelope.data.commands;

describe("droppedOf", () => {
  it("splits a transpiler warning into client and field", () => {
    expect(droppedOf("cursor: 'tools' field not supported, dropped")).toEqual({
      client: "cursor",
      field: "tools",
      text: "cursor: 'tools' field not supported, dropped",
    });
    expect(
      droppedOf("codex-cli: 'tools' field not supported in agent TOML, dropped"),
    ).toMatchObject({ client: "codex-cli", field: "tools" });
  });

  it("keeps a line of any other shape as text", () => {
    expect(droppedOf("agents/x/AGENT.md: bad front matter")).toMatchObject({
      client: null,
      field: null,
    });
  });

  it("matches a vscode warning to the vscode-copilot column only", () => {
    const d = droppedOf("vscode: 'max-turns' not supported, dropped");
    expect(warnsClient(d, "vscode-copilot")).toBe(true);
    expect(warnsClient(d, "cursor")).toBe(false);
    expect(warnsClient(droppedOf("cursor: 'tools' dropped"), "cursor")).toBe(true);
  });
});

describe("policyOf", () => {
  it("reads tier and preview flag from the registry", () => {
    expect(policyOf(rows, "agents sync")).toEqual({
      tier: "write",
      previewFlag: "--dry-run",
      terminal: false,
    });
    expect(policyOf(rows, "agents clean")?.tier).toBe("destructive");
    expect(policyOf(rows, "agents uninstall")?.tier).toBe("destructive");
    expect(policyOf(rows, "styles remove")?.tier).toBe("destructive");
    expect(policyOf(rows, "styles clean")?.tier).toBe("destructive");
    expect(policyOf(rows, "styles apply")?.tier).toBe("write");
    expect(policyOf(rows, "agents ls")?.previewFlag).toBeNull();
  });

  it("has no policy for what the registry does not know", () => {
    expect(policyOf(rows, "agents frobnicate")).toBeNull();
    expect(policyOf(null, "agents sync")).toBeNull();
  });
});

describe("the fixture rows", () => {
  it("carry the tier and preview flag of the golden registry", () => {
    for (const fixture of agentsCommandRows) {
      const real = policyOf(rows, fixture.id)!;
      expect(policyOf(agentsCommandRows, fixture.id), fixture.id).toEqual(real);
    }
  });
});

describe("nameProblem", () => {
  it("accepts a plain name and explains the rest", () => {
    expect(nameProblem("scout")).toBeNull();
    expect(nameProblem("code-reviewer_2")).toBeNull();
    expect(nameProblem("")).toMatch(/name/);
    expect(nameProblem("Has Space")).toMatch(/lowercase/);
    expect(nameProblem("../etc")).toMatch(/lowercase/);
  });
});

describe("shortPath", () => {
  it("keeps the last three parts of a long path", () => {
    expect(shortPath("/a/b/c/d/e.md")).toBe("…/c/d/e.md");
    expect(shortPath("/a/b")).toBe("/a/b");
  });
});

describe("planOf over the golden envelopes", () => {
  it("words agents sync as one create step per output file, with the warnings", () => {
    const plan = planOf("agents sync", golden("agents-sync.preview"), false)!;
    expect(plan.summary).toBe("Write 1 agent to 4 clients");
    expect(plan.steps.map((s) => s.path)).toEqual([
      "<WORLD>/home/.claude/agents/helper.md",
      "<WORLD>/home/.codex/agents/helper.toml",
      "<WORLD>/home/.cursor/agents/helper.md",
      "<WORLD>/home/.gemini/agents/helper.md",
    ]);
    expect(plan.undo).toBe("toolportctl agents clean");
    expect(planOf("agents sync", golden("agents-sync.apply"), true)!.summary).toMatch(
      /^Wrote /,
    );
  });

  it("carries the dropped-field warnings of an agent into the plan", () => {
    const data = golden("agents-sync.preview");
    data.agents[0].warnings = ["cursor: 'tools' field not supported, dropped"];
    expect(planOf("agents sync", data, false)!.warnings).toEqual([
      "helper: cursor: 'tools' field not supported, dropped",
    ]);
  });

  it.each([
    ["agents clean", "agents-clean.preview", "Remove 4 synced agent files"],
    ["styles clean", "styles-clean.preview", "Remove 2 style files"],
  ])("words %s as deletes", (command, stem, summary) => {
    const plan = planOf(command, golden(stem), false)!;
    expect(plan.summary).toBe(summary);
    expect(plan.steps.filter((s) => s.op === "delete").length).toBeGreaterThan(1);
  });

  it("words agents uninstall as the source, its outputs and the lock entry", () => {
    const plan = planOf("agents uninstall", golden("agents-uninstall.preview"), false)!;
    expect(plan.steps[0]).toMatchObject({
      op: "delete",
      path: expect.stringContaining("helper"),
    });
    expect(plan.steps.at(-1)?.op).toBe("update");
    expect(plan.steps).toHaveLength(6);
  });

  it("words styles apply and remove per client", () => {
    const apply = planOf("styles apply", golden("styles-apply.preview"), false)!;
    expect(apply.summary).toBe("Apply 'plain' as an always-on rule in 13 clients");
    expect(apply.undo).toBe("toolportctl styles remove");
    const remove = planOf("styles remove", golden("styles-remove.preview"), false)!;
    expect(remove.steps).toHaveLength(13);
    expect(remove.undo).toBe("toolportctl styles apply plain");
  });

  it.each([
    ["agents add", "agents-add.preview", "agent 'scout'"],
    ["styles add", "styles-add.preview", "style 'terse'"],
  ])("words %s as one new file", (command, stem, text) => {
    const plan = planOf(command, golden(stem), false)!;
    expect(plan.summary).toContain(text);
    expect(plan.steps).toHaveLength(1);
    expect(plan.steps[0].op).toBe("create");
  });

  it("returns null for a command it does not word", () => {
    expect(planOf("agents ls", {}, false)).toBeNull();
  });
});
