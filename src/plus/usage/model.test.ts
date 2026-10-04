import { describe, expect, it } from "vitest";
import {
  addDays,
  axisOf,
  compact,
  enableArgv,
  formatTs,
  latestTs,
  otelPlan,
  parsePort,
  parseStatus,
  parseUsage,
  percent,
  policyOf,
  tokensOf,
  usageArgv,
  windowOf,
} from "./model";
import { goldenData, registry } from "./testkit";
import { OTEL_ONLY_MESSAGE, aggregate, usageWorld, worldMessages } from "./world";
import type { CommandRow } from "../bridge/data";

describe("parseUsage: the golden usage envelope", () => {
  const view = parseUsage(goldenData("usage.apply"));

  it("keeps every number of `toolportctl usage` as it is", () => {
    expect(view.totals).toEqual({
      messages: 6,
      input: 60,
      output: 240,
      cacheCreation: 3000,
      cacheRead: 42000,
    });
    expect(tokensOf(view.totals)).toBe(45_300);
    expect(view.days).toEqual([
      {
        day: "2026-10-01",
        messages: 6,
        input: 60,
        output: 240,
        cacheCreation: 3000,
        cacheRead: 42000,
      },
    ]);
    expect(view.servers).toEqual([
      { name: "github", calls: 3, tools: [{ name: "mcp__github__list", calls: 3 }] },
    ]);
    expect(view.sessions).toHaveLength(1);
    expect(view.sessions[0]).toMatchObject({ id: "s1", cwd: "/work/demo", messages: 6 });
    expect(view.index).toEqual({ files: 1, messages: 6 });
    expect(view.sources).toEqual({ otelOnly: 0, otelRequests: 0, transcriptMessages: 6 });
  });

  it("groups the sessions by their folder into projects", () => {
    expect(view.projects).toEqual([
      expect.objectContaining({ cwd: "/work/demo", name: "demo", sessions: 1 }),
    ]);
  });

  it("reads the same numbers from the cached envelope", () => {
    expect(parseUsage(goldenData("usage.cached"))).toEqual(view);
  });
});

describe("parseUsage: the synthetic transcript index", () => {
  const messages = worldMessages();
  const view = parseUsage(usageWorld());

  it("adds a message once however often its id appears", () => {
    expect(aggregate([...messages, ...messages])).toEqual(aggregate(messages));
  });

  it("totals what the messages hold, the OTel-only request included", () => {
    const all = [...messages, OTEL_ONLY_MESSAGE];
    const sum = (key: "input" | "output" | "cacheCreation" | "cacheRead") =>
      all.reduce((total, msg) => total + msg[key], 0);
    expect(view.totals).toEqual({
      messages: all.length,
      input: sum("input"),
      output: sum("output"),
      cacheCreation: sum("cacheCreation"),
      cacheRead: sum("cacheRead"),
    });
  });

  it("splits the projects so that they add up to the totals", () => {
    expect(view.projects.map((project) => project.name)).toContain("acme-erp");
    const sessions = view.projects.reduce((n, project) => n + project.sessions, 0);
    expect(sessions).toBe(view.sessions.length);
    expect(view.projects.reduce((n, project) => n + tokensOf(project), 0)).toBe(
      tokensOf(view.totals),
    );
    const tokens = view.projects.map(tokensOf);
    expect(tokens).toEqual([...tokens].sort((a, b) => b - a));
  });

  it("orders the sessions by tokens, the most first", () => {
    const tokens = view.sessions.map(tokensOf);
    expect(tokens).toEqual([...tokens].sort((a, b) => b - a));
  });

  it("finds the newest message across the sessions", () => {
    expect(latestTs(view)).toBe("2026-10-03T16:30:00Z");
  });

  it("tolerates a missing member", () => {
    const empty = parseUsage({});
    expect(empty.totals.messages).toBe(0);
    expect(empty.days).toEqual([]);
    expect(empty.projects).toEqual([]);
  });

  it("gives a session without a folder a project of its own", () => {
    const view = parseUsage({ bySession: { s9: { messages: 1, input: 5, cwd: "" } } });
    expect(view.projects[0]).toMatchObject({ name: "(no folder recorded)", sessions: 1 });
  });
});

describe("windows and axes", () => {
  it("counts days across a month end", () => {
    expect(addDays("2026-10-01", -1)).toBe("2026-09-30");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
  });

  it("fills a window with a row for every day, zeros where nothing happened", () => {
    const rows = windowOf(
      [
        {
          day: "2026-10-03",
          messages: 2,
          input: 1,
          output: 2,
          cacheCreation: 3,
          cacheRead: 4,
        },
      ],
      "2026-10-04",
      7,
    );
    expect(rows.map((row) => row.day)).toEqual([
      "2026-09-28",
      "2026-09-29",
      "2026-09-30",
      "2026-10-01",
      "2026-10-02",
      "2026-10-03",
      "2026-10-04",
    ]);
    expect(rows[5].messages).toBe(2);
    expect(rows[6]).toMatchObject({ messages: 0, input: 0 });
  });

  it("has no window for a today that is not a date", () => {
    expect(windowOf([], "soon", 7)).toEqual([]);
  });

  it("rounds the top of an axis up to a round number", () => {
    expect(axisOf(45_300).top).toBe(50_000);
    expect(axisOf(45_300).ticks.map((tick) => tick.label)).toEqual(["0", "25K", "50K"]);
    expect(axisOf(8).top).toBe(10);
    expect(axisOf(3).top).toBe(4);
    expect(axisOf(0).top).toBe(1);
    expect(axisOf(1_200_000).top).toBe(2_000_000);
  });
});

describe("formatting", () => {
  it("shortens big numbers and keeps the small ones", () => {
    expect(compact(950)).toBe("950");
    expect(compact(45_300)).toBe("45.3K");
    expect(compact(71_300_000)).toBe("71.3M");
    expect(compact(2_100_000_000)).toBe("2.1B");
    expect(compact(123_456)).toBe("123K");
  });

  it("writes a share as a percentage, or says there is none", () => {
    expect(percent(42_000 / 45_300)).toBe("92.7%");
    expect(percent(1)).toBe("100%");
    expect(percent(0)).toBe("0%");
    expect(percent(null)).toBe("n/a");
  });

  it("writes a time in UTC and leaves anything else as it is", () => {
    expect(formatTs("2026-10-01T10:05:09Z")).toBe("2026-10-01 10:05 UTC");
    expect(formatTs("<TIME>")).toBe("<TIME>");
    expect(formatTs("")).toBe("unknown");
  });
});

describe("commands", () => {
  it("builds the argv of the golden envelopes", () => {
    expect(usageArgv({ refresh: true, root: "" })).toEqual(["usage"]);
    expect(usageArgv({ refresh: false, root: "" })).toEqual(["usage", "--no-refresh"]);
    expect(usageArgv({ refresh: false, root: "/x" })).toEqual([
      "usage",
      "--no-refresh",
      "--root",
      "/x",
    ]);
    expect(enableArgv(4999)).toEqual(["obs", "otel", "enable", "--port", "4999"]);
  });

  it("takes a port from 1 to 65535 and nothing else", () => {
    expect(parsePort("4318")).toBe(4318);
    expect(parsePort(" 65535 ")).toBe(65535);
    for (const bad of ["", "0", "65536", "-1", "80.5", "port", "1e3", "123456"])
      expect(parsePort(bad)).toBeNull();
  });

  it("reads the tier and the preview flag from the registry", () => {
    const rows = (registry() as { commands: CommandRow[] }).commands;
    expect(policyOf(rows, "obs otel enable")).toEqual({
      tier: "write",
      previewFlag: "--dry-run",
      terminal: false,
    });
    expect(policyOf(rows, "usage")).toMatchObject({ tier: "read", previewFlag: null });
    expect(policyOf(rows, "obs otel")).toBeNull();
    expect(policyOf(null, "usage")).toBeNull();
  });
});

describe("OTel plans", () => {
  it("words the enable dry run with the CLI's own lines", () => {
    const data = goldenData("obs-otel-enable.preview");
    const plan = otelPlan(data, "enable", "toolportctl obs otel disable");
    expect(plan?.steps.map((step) => step.detail)).toEqual(data.actions);
    expect(plan?.steps.every((step) => step.op === "create")).toBe(true);
    expect(plan?.steps[0].path).toBe(data.settingsPath);
    expect(plan?.summary).toContain("http://127.0.0.1:4999");
    expect(plan?.undo).toBe("toolportctl obs otel disable");
  });

  it("words the disable dry run, and warns about a key that stays", () => {
    const data = {
      ...goldenData("obs-otel-disable.preview"),
      kept: ["OTEL_LOGS_EXPORTER"],
    };
    const plan = otelPlan(data, "disable", "undo");
    expect(plan?.steps.every((step) => step.op === "delete")).toBe(true);
    expect(plan?.warnings).toEqual([
      "env.OTEL_LOGS_EXPORTER stays in your settings: you changed it after Toolport set it",
    ]);
  });

  it("is no plan when the data has no action lines", () => {
    expect(otelPlan({}, "enable", "")).toBeNull();
  });
});

describe("parseStatus", () => {
  it("reads the golden status and nothing it does not name", () => {
    const status = parseStatus({
      ...goldenData("obs-otel-status"),
      headers: "CANARY",
    });
    expect(status).toMatchObject({
      enabled: false,
      endpoint: "http://127.0.0.1:4318",
      port: 4318,
      events: { count: 0, latest: null },
      receiver: { state: "disabled", listening: false, error: null },
    });
    expect(status.settings.keys).toHaveLength(5);
    expect(JSON.stringify(status)).not.toContain("CANARY");
  });
});
