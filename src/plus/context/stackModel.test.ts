import { describe, expect, it } from "vitest";
import type { LoadItem, LoadsData, MeasureData } from "../bridge/data";
import { goldenData } from "./testkit";
import {
  STACK_GROUPS,
  basisWord,
  biggestPlugin,
  groupOf,
  measuredView,
  skillBudgetState,
  stackGroups,
  tokenLabel,
  withoutSpec,
} from "./stackModel";

const folder = () => goldenData("context-loads.folder") as LoadsData;
const item = (patch: Partial<LoadItem>): LoadItem => ({
  basis: "estimate",
  kind: "memory",
  lazy: false,
  loaded: true,
  name: "CLAUDE.md",
  origin: { kind: "repo", name: "erp" },
  path: null,
  reason: "",
  scope: "always",
  source: "project",
  tokens: 10,
  via: [],
  visible: null,
  writable: false,
  ...patch,
});

describe("groupOf", () => {
  it("puts on-demand rows and never-loaded rows apart, whatever their kind", () => {
    expect(groupOf(item({ kind: "skill", lazy: true, loaded: false }))).toBe(
      "Loads on demand",
    );
    expect(groupOf(item({ kind: "memory", lazy: false, loaded: false }))).toBe(
      "Not loaded here",
    );
  });

  it("groups a loaded row by its kind", () => {
    const kinds: Array<[LoadItem["kind"], string]> = [
      ["memory", "Instructions"],
      ["import", "Instructions"],
      ["rule", "Instructions"],
      ["skill", "Skills, commands and agents"],
      ["command", "Skills, commands and agents"],
      ["agent", "Skills, commands and agents"],
      ["plugin", "Plugins"],
      ["mcp", "Tools (MCP)"],
      ["memory-index", "Memory"],
      ["settings", "Settings"],
    ];
    for (const [kind, group] of kinds) expect(groupOf(item({ kind }))).toBe(group);
  });
});

describe("stackGroups", () => {
  it("lists the groups in load order and leaves out the empty ones", () => {
    const groups = stackGroups(folder()).map((view) => view.group);
    expect(groups).toEqual(STACK_GROUPS.filter((group) => groups.includes(group)));
    expect(groups[0]).toBe("Instructions");
    expect(groups.at(-1)).toBe("Not loaded here");
    expect(stackGroups({ ...folder(), items: [] })).toEqual([]);
  });

  it("orders a group by size and sums its tokens", () => {
    const data = {
      ...folder(),
      items: [item({ name: "a", tokens: 5 }), item({ name: "b", tokens: 50 })],
    };
    const [view] = stackGroups(data);
    expect(view.items.map((row) => row.name)).toEqual(["b", "a"]);
    expect(view.tokens).toBe(55);
  });

  it("counts every row exactly once", () => {
    const data = folder();
    const total = stackGroups(data).reduce((sum, view) => sum + view.items.length, 0);
    expect(total).toBe(data.items.length);
  });

  it("keeps the first groups equal to what loads at the start", () => {
    const data = folder();
    const groups = stackGroups(data);
    const start = groups
      .filter(
        (view) => view.group !== "Loads on demand" && view.group !== "Not loaded here",
      )
      .reduce((sum, view) => sum + view.tokens, 0);
    expect(start).toBe(data.total_tokens);
  });
});

describe("biggestPlugin", () => {
  it("is the loaded plugin with the most tokens, never one that is off", () => {
    expect(biggestPlugin(folder())?.name).toBe("kit@market");
    expect(
      biggestPlugin({
        ...folder(),
        items: [
          item({ kind: "plugin", name: "off@m", tokens: 900, loaded: false }),
          item({ kind: "plugin", name: "on@m", tokens: 3 }),
        ],
      })?.name,
    ).toBe("on@m");
  });

  it("is null without a loaded plugin", () => {
    expect(biggestPlugin({ ...folder(), items: [item({})] })).toBeNull();
  });
});

describe("labels", () => {
  it("names the basis of every number and defaults to estimate", () => {
    expect(basisWord("measured")).toBe("measured");
    expect(basisWord("projected")).toBe("projected");
    expect(basisWord("estimate")).toBe("estimate");
    expect(basisWord("anything else")).toBe("estimate");
    expect(tokenLabel(10070, "estimate")).toBe("10,070 tokens, estimate");
    expect(tokenLabel(68445, "measured")).toBe("68,445 tokens, measured");
  });

  it("builds the --without argument of a plugin", () => {
    expect(withoutSpec(item({ kind: "plugin", name: "kit@market" }))).toBe(
      "plugin:kit@market",
    );
  });
});

describe("measuredView", () => {
  const measured = () => goldenData("context-measure.measured") as MeasureData;

  it("takes the as-is run, the signed deltas and the skills Claude Code does not list", () => {
    const view = measuredView(measured());
    expect(view.asIs?.label).toBe("as is");
    expect(view.asIs?.total).toBe(68445);
    expect(view.deltas).toEqual([
      { label: "without plugin:kit@market", percent: -12.6, tokens: -8651 },
    ]);
    expect(view.invisible.map((skill) => skill.name)).toEqual(["handoff"]);
  });

  it("falls back to the first run, or none", () => {
    const data = measured();
    expect(
      measuredView({ ...data, runs: data.runs.map((run) => ({ ...run, label: "x" })) })
        .asIs?.label,
    ).toBe("x");
    expect(measuredView({ ...data, runs: [] }).asIs).toBeNull();
  });
});

describe("skillBudgetState", () => {
  it("reports the budget and which skills lose their description", () => {
    const state = skillBudgetState(folder());
    expect(state).toMatchObject({ used: 1998, limit: 2000, over: true });
    expect(state.capped).toHaveLength(17);
  });

  it("is not over when nothing is capped", () => {
    const data = folder();
    expect(
      skillBudgetState({ ...data, skill_budget: { ...data.skill_budget, capped: [] } })
        .over,
    ).toBe(false);
  });
});
