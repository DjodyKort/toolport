import { describe, expect, it } from "vitest";
import type { EntryHealth, HookCounter, HookEntry, HooksStatusData } from "./data";
import {
  dayRange,
  headline,
  hookRows,
  intervalText,
  latencyText,
  overall,
  providerTiles,
  untilText,
  windowText,
} from "./model";

const FILE = "/Users/example/.claude/settings.json";

function entry(
  provider: "rtk" | "read-once",
  health: EntryHealth,
  file = FILE,
): HookEntry {
  return {
    file,
    provider,
    state: health === "absent" || health === "missing" ? "absent" : "ours",
    command: health === "absent" ? null : "cmd",
    health,
    wrappedAt: health === "ok" ? "2026-11-05T09:00:00.000Z" : null,
  };
}

const counter = (extra: Partial<HookCounter> = {}): HookCounter => ({
  provider: "rtk",
  basis: "mechanical",
  calls: 2100,
  answered: 1842,
  failed: 0,
  medianLatencyMs: 38,
  p95LatencyMs: 140,
  maxLatencyMs: 400,
  firstCall: "2026-11-05",
  lastCall: "2026-11-16",
  ...extra,
});

function status(
  rtk: EntryHealth,
  readOnce: EntryHealth,
  extra: Partial<HooksStatusData> = {},
): HooksStatusData {
  return {
    paused: false,
    pausedSince: null,
    pausedUntil: null,
    entries: [entry("rtk", rtk), entry("read-once", readOnce)],
    counters: [counter(), counter({ provider: "read-once", calls: 0 })],
    wrapped: 0,
    epochs: 0,
    cannotTell: "",
    trust: [
      {
        provider: "rtk-only",
        trust: {
          closedEngine: false,
          installsGlobalHooks: true,
          needsEgressConsent: false,
          sendsCodeOffMachine: false,
        },
      },
      {
        provider: "headroom",
        trust: {
          closedEngine: false,
          installsGlobalHooks: false,
          needsEgressConsent: false,
          sendsCodeOffMachine: false,
        },
      },
      {
        provider: "parsec",
        trust: {
          closedEngine: true,
          installsGlobalHooks: true,
          needsEgressConsent: true,
          sendsCodeOffMachine: true,
        },
      },
    ],
    ...extra,
  };
}

describe("hook rows", () => {
  it("test01_the_worst_health_among_several_files_wins_and_an_absent_file_says_nothing", () => {
    expect(
      overall([entry("rtk", "ok"), entry("rtk", "absent", "/b/settings.json")]),
    ).toBe("ok");
    expect(
      overall([entry("rtk", "ok"), entry("rtk", "foreign", "/b/settings.json")]),
    ).toBe("foreign");
    expect(
      overall([entry("rtk", "edited"), entry("rtk", "missing", "/b/settings.json")]),
    ).toBe("missing");
    expect(overall([entry("rtk", "absent")])).toBe("absent");
    expect(overall([])).toBe("absent");
  });

  it("test02_a_row_counts_the_files_that_hold_the_entry", () => {
    const data = status("ok", "absent", {
      entries: [
        entry("rtk", "ok"),
        entry("rtk", "ok", "/b/settings.json"),
        entry("rtk", "absent", "/c/settings.json"),
        entry("read-once", "absent"),
      ],
    });
    const [rtk, readOnce] = hookRows(data);
    expect(rtk).toMatchObject({ health: "ok", files: 2, wrappedAt: expect.any(String) });
    expect(readOnce).toMatchObject({ health: "absent", files: 0, command: null });
  });
});

describe("the leading card", () => {
  const kind = (rtk: EntryHealth, readOnce: EntryHealth, paused = false) =>
    headline(hookRows(status(rtk, readOnce)), paused).kind;

  it("test03_something_missing_or_edited_comes_before_everything_else", () => {
    expect(kind("missing", "foreign", true)).toBe("attention");
    expect(kind("ok", "edited")).toBe("attention");
  });

  it("test04_paused_needs_a_hook_of_ours_and_adopting_comes_before_the_plain_state", () => {
    expect(kind("ok", "ok", true)).toBe("paused");
    expect(kind("foreign", "absent", true)).toBe("adopt");
    expect(kind("ok", "foreign")).toBe("adopt");
    expect(kind("ok", "ok")).toBe("on");
    expect(kind("absent", "absent")).toBe("none");
  });

  it("test05_the_card_names_the_providers_it_is_about", () => {
    const head = headline(hookRows(status("foreign", "foreign")), false);
    expect(head.foreign).toEqual(["rtk", "read-once"]);
    expect(headline(hookRows(status("ok", "ok")), false).owned).toBe(2);
  });
});

describe("counted figures", () => {
  it("test06_a_hook_with_no_calls_says_counting_starts_with_the_next_session", () => {
    expect(latencyText(null)).toContain("next Claude Code session");
    expect(latencyText(counter({ calls: 0 }))).toContain("next Claude Code session");
  });

  it("test07_the_figures_read_calls_median_and_the_95th_percentile_at_the_hook", () => {
    expect(latencyText(counter())).toBe(
      "Answered 1,842 of 2,100 calls since 2026-11-05 · median 38 ms · p95 140 ms (counted at the hook)",
    );
    expect(latencyText(counter({ failed: 3, p95LatencyMs: null }))).toContain(
      "· median 38 ms · 3 failed",
    );
  });
});

describe("provider tiles", () => {
  it("test08_the_hook_tiles_follow_the_entries_and_a_pause_shows_on_the_healthy_ones", () => {
    const states = (data: HooksStatusData) =>
      providerTiles(data, null)
        .slice(0, 2)
        .map((t) => t.state);
    expect(states(status("ok", "foreign"))).toEqual(["active", "not-managed"]);
    expect(states(status("ok", "absent", { paused: true }))).toEqual([
      "paused",
      "not-found",
    ]);
    expect(states(status("edited", "missing"))).toEqual(["attention", "attention"]);
  });

  it("test09_the_proxy_is_off_and_parsec_locked_until_the_policy_chooses_them", () => {
    const tiles = providerTiles(status("ok", "ok"), null);
    expect(tiles.map((t) => [t.id, t.state])).toEqual([
      ["rtk", "active"],
      ["read-once", "active"],
      ["headroom", "off"],
      ["parsec", "locked"],
    ]);
    expect(tiles[3].trust).toBe(
      "Closed engine · sends your code off this machine · installs a global hook entry",
    );
    expect(tiles[2].trust).toBe("Open source · runs locally · changes the API route");
    expect(tiles[0].trust).toBe(
      "Open source · runs locally · installs a global hook entry",
    );
  });
});

describe("measurement text", () => {
  it("test10_a_window_reads_sessions_median_and_the_middle_half", () => {
    expect(
      windowText({ sessions: 118, relative: 100, relativeLow: 71, relativeHigh: 160 }),
    ).toBe("118 sessions · median cost 100 · middle half 71 to 160");
    expect(
      windowText({ sessions: 1, relative: null, relativeLow: null, relativeHigh: null }),
    ).toBe("1 session · no hooks-on window to compare with");
    expect(
      windowText({ sessions: 0, relative: null, relativeLow: null, relativeHigh: null }),
    ).toBe("No sessions with enough turns");
  });

  it("test11_intervals_and_day_ranges_name_open_ends", () => {
    expect(intervalText({ from: null, to: "2026-11-05" })).toBe("before 2026-11-05");
    expect(intervalText({ from: "2026-11-13", to: null })).toBe("since 2026-11-13");
    expect(intervalText({ from: "2026-11-05", to: "2026-11-12" })).toBe(
      "2026-11-05 to 2026-11-12",
    );
    expect(dayRange("2026-10-16", "2026-10-16")).toBe("2026-10-16");
    expect(dayRange(null, null)).toBe("no dates");
  });
});

describe("untilText", () => {
  const now = new Date(2026, 10, 17, 10, 0);
  it("test40_says_today_tomorrow_or_the_day", () => {
    expect(untilText(new Date(2026, 10, 17, 21, 5).toISOString(), now)).toBe(
      "today 21:05",
    );
    expect(untilText(new Date(2026, 10, 18, 9, 12).toISOString(), now)).toBe(
      "tomorrow 09:12",
    );
    expect(untilText(new Date(2026, 10, 23, 9, 12).toISOString(), now)).toBe(
      "Mon 23 Nov 09:12",
    );
    expect(untilText("not a time", now)).toBe("not a time");
  });
});
