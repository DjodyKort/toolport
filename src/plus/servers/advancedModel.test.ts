import { describe, expect, it } from "vitest";
import {
  bridgeWith,
  changedCount,
  editable,
  minutesText,
  parseLimit,
  span,
  summaryLine,
  toolsLine,
  type Values,
} from "./advancedModel";
import type { InteropData, ServerInfoData } from "./data";

const interop: InteropData = {
  bridgeCeilingMs: 3_660_000,
  bridgeSetBy: "bravo-build",
  otherLongest: null,
  defaultLimitMs: 3_600_000,
  defaultRequestTimeoutMs: 30_000,
  bridgeBaseMs: 600_000,
  bridgeGraceMs: 60_000,
  pendingRedeclare: false,
};

function info(extra: Partial<ServerInfoData> = {}): ServerInfoData {
  return {
    id: "bravo",
    name: "bravo-build",
    enabled: true,
    transport: "stdio",
    command: "bravo-mcp",
    args: [],
    url: null,
    cwd: null,
    env: [],
    inheritEnv: false,
    mode: "auto",
    declareClientCapabilities: false,
    forwardInstructions: false,
    maxRequestTimeoutMs: null,
    requestTimeoutMs: null,
    interop,
    disabledTools: [],
    source: null,
    accessSets: [],
    ...extra,
  };
}

const defaults: Values = {
  mode: "auto",
  declareClientCapabilities: false,
  forwardInstructions: false,
  limitMs: null,
};

describe("the call limit field", () => {
  it("test01_empty_means_the_default_and_a_number_is_whole_or_decimal_minutes", () => {
    expect(parseLimit("")).toEqual({ kind: "default" });
    expect(parseLimit("  ")).toEqual({ kind: "default" });
    expect(parseLimit("90")).toEqual({ kind: "ok", ms: 5_400_000 });
    expect(parseLimit("1.5")).toEqual({ kind: "ok", ms: 90_000 });
    expect(parseLimit("1,5")).toEqual({ kind: "ok", ms: 90_000 });
    expect(parseLimit("1")).toEqual({ kind: "ok", ms: 60_000 });
    expect(parseLimit("1440")).toEqual({ kind: "ok", ms: 86_400_000 });
  });

  it("test02_anything_else_is_explained_never_clamped", () => {
    expect(parseLimit("ninety")).toMatchObject({
      kind: "error",
      message: "Enter a number of minutes, for example 90.",
    });
    expect(parseLimit("-5")).toMatchObject({ kind: "error" });
    expect(parseLimit("1e3")).toMatchObject({ kind: "error" });
    expect(parseLimit("0.5")).toMatchObject({
      kind: "error",
      message: expect.stringContaining("At least 1 minute"),
    });
    expect(parseLimit("1441")).toEqual({
      kind: "error",
      message: "At most 1,440 minutes (24 hours).",
    });
  });

  it("test03_a_stored_limit_is_shown_in_minutes_and_one_under_a_minute_stays_read_only", () => {
    expect(minutesText(5_400_000)).toBe("90");
    expect(minutesText(90_000)).toBe("1.5");
    expect(editable(null)).toBe(true);
    expect(editable(60_000)).toBe(true);
    expect(editable(59_999)).toBe(false);
  });

  it("test04_a_span_reads_in_the_largest_unit_that_fits", () => {
    expect(span(45_000)).toBe("45 s");
    expect(span(90_000)).toBe("1 min 30 s");
    expect(span(1_800_000)).toBe("30 min");
    expect(span(3_600_000)).toBe("1 h");
    expect(span(3_660_000)).toBe("1 h 1 min");
    expect(span(5_400_000)).toBe("1 h 30 min");
  });
});

describe("the bridge ceiling", () => {
  it("test05_follows_the_longest_limit_among_enabled_servers_plus_a_minute", () => {
    const own = info();
    expect(bridgeWith(own, null, own.name)).toEqual({
      ceilingMs: 3_660_000,
      setBy: "bravo-build",
    });
    expect(bridgeWith(own, 10_800_000, own.name).ceilingMs).toBe(10_860_000);
  });

  it("test06_names_the_other_server_while_it_keeps_the_longest_wait", () => {
    const crowded = info({
      interop: { ...interop, otherLongest: { name: "delta-sync", ceilingMs: 7_200_000 } },
    });
    expect(bridgeWith(crowded, 5_400_000, "bravo-build")).toEqual({
      ceilingMs: 7_260_000,
      setBy: "delta-sync",
    });
    expect(bridgeWith(crowded, 10_800_000, "bravo-build")).toEqual({
      ceilingMs: 10_860_000,
      setBy: "bravo-build",
    });
  });

  it("test07_a_disabled_server_or_a_short_limit_leaves_the_bridge_at_its_base", () => {
    expect(bridgeWith(info({ enabled: false }), null, "bravo-build")).toEqual({
      ceilingMs: 600_000,
      setBy: null,
    });
    expect(
      bridgeWith(info({ requestTimeoutMs: 60_000 }), 120_000, "bravo-build"),
    ).toEqual({ ceilingMs: 600_000, setBy: null });
  });

  it("test08_a_remote_server_only_counts_its_request_timeout", () => {
    const remote = info({ command: null, transport: "http", url: "https://x.invalid" });
    expect(bridgeWith(remote, 10_800_000, "bravo-build").ceilingMs).toBe(600_000);
  });
});

describe("the collapsed line", () => {
  it("test09_says_defaults_when_nothing_differs", () => {
    expect(summaryLine(defaults, interop, true)).toBe(
      "Defaults · all tools listed · no interop switches · calls up to 1 h",
    );
    expect(changedCount(defaults)).toBe(0);
  });

  it("test10_lists_what_differs_in_the_order_of_the_open_row", () => {
    const changed: Values = {
      mode: "search",
      declareClientCapabilities: true,
      forwardInstructions: true,
      limitMs: 5_400_000,
    };
    expect(summaryLine(changed, interop, true)).toBe(
      "Search only · 2 switches on · calls up to 1 h 30 min",
    );
    expect(changedCount(changed)).toBe(4);
    expect(summaryLine({ ...defaults, forwardInstructions: true }, interop, false)).toBe(
      "All tools listed · 1 switch on",
    );
  });

  it("test11_the_tool_line_says_what_is_listed_or_what_leaves", () => {
    expect(toolsLine("auto", 31)).toContain("31 tools in the list");
    expect(toolsLine("auto", null)).toBe(
      "Its tools are listed to every client, like any other server's.",
    );
    expect(toolsLine("search", 512)).toMatch(/^Its 512 tools are left out of the list/);
  });
});
