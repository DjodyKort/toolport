import { describe, expect, it } from "vitest";
import {
  createSpendWorld,
  SPEND_NOW,
  PROJECT_WEB,
  type SpendWorldOptions,
} from "../fixtures/spendWorld";
import type { SpendRow, StatusData, SummaryData } from "./data";
import {
  age,
  bars,
  breakdown,
  cacheLines,
  chartTable,
  collectingCard,
  delta,
  filterNote,
  legend,
  mcpCard,
  money,
  moneyShort,
  monthDay,
  NO_FILTER,
  notices,
  progress,
  sessionLines,
  spendState,
  staleCauses,
  strip,
  tildePath,
  tiles,
  toggleProject,
  tokens,
} from "./model";

function read(options: SpendWorldOptions = {}, ...argv: string[]) {
  const world = createSpendWorld(options);
  const data = (words: string[]) => {
    const reply = world.run(["spend", ...words]);
    if (!reply.ok) throw new Error(reply.error?.message);
    return reply.data;
  };
  return {
    status: data(["status"]) as StatusData,
    summary: data(["summary", ...argv]) as SummaryData,
  };
}

describe("figures", () => {
  it("test01_money_and_tokens_use_the_units_of_the_design", () => {
    expect(money(31.4)).toBe("$31.40");
    expect([164.2, 72.4, 0.3, 11.24].map(moneyShort)).toEqual([
      "$164",
      "$72.4",
      "$0.30",
      "$11.2",
    ]);
    expect([108_700_000, 2_470_000_000, 4100, 640].map((n) => tokens(n))).toEqual([
      "108.7M",
      "2.47B",
      "4.1k",
      "640",
    ]);
    expect(tokens(8_120_000, 2)).toBe("8.12M");
  });

  it("test02_deltas_dates_and_ages_read_the_way_the_page_says_them", () => {
    expect([0.12, -0.05, 0, null].map(delta)).toEqual(["+12%", "-5%", "±0%", null]);
    expect(monthDay("2026-11-04")).toBe("11-04");
    expect(monthDay("2026-11-04T23:59:00Z")).toBe("11-04");
    const now = SPEND_NOW;
    expect(age(new Date(now - 14_000).toISOString(), now)).toBe("14 s ago");
    expect(age(new Date(now - 3 * 3_600_000).toISOString(), now)).toBe("3 h ago");
    expect(age(null, now)).toBeNull();
    expect(tildePath("/Users/example/work/a")).toBe("~/work/a");
    expect(tildePath("/work/a")).toBe("/work/a");
  });
});

describe("the state machine", () => {
  const stateOf = (options: SpendWorldOptions, ready = true) => {
    const { status } = read(options);
    return spendState(status, ready);
  };

  it("test03_each_scenario_lands_in_its_state", () => {
    expect(stateOf({ scenario: "never" })).toBe("neverSetUp");
    expect(stateOf({ scenario: "off" })).toBe("offWithTranscripts");
    expect(stateOf({ scenario: "waiting" })).toBe("waitingFirstEvent");
    expect(stateOf({ scenario: "cannot-listen" })).toBe("receiverCannotListen");
    expect(stateOf({ scenario: "healthy" })).toBe("healthy");
  });

  it("test04_computing_is_the_first_read_of_transcripts_before_any_report", () => {
    expect(stateOf({ scenario: "computing" }, false)).toBe("computing");
    expect(stateOf({ scenario: "computing" }, true)).toBe("offWithTranscripts");
    expect(stateOf({ scenario: "healthy" }, false)).toBe("healthy");
  });

  it("test05_a_listener_that_is_gone_is_the_error_state_not_a_quiet_off", () => {
    const { status } = read({ scenario: "healthy" });
    const gone: StatusData = {
      ...status,
      state: "not-listening",
      receiver: { ...status.receiver, listening: false },
    };
    expect(spendState(gone, true)).toBe("receiverCannotListen");
  });

  it("test06_stored_data_with_collecting_off_is_not_a_first_run", () => {
    const { status } = read({ scenario: "never" });
    const kept: StatusData = {
      ...status,
      store: { ...status.store, events: 10 },
    };
    expect(spendState(kept, true)).toBe("offWithTranscripts");
  });
});

describe("the strip and its notices", () => {
  it("test07_the_strip_names_the_receiver_and_the_last_event_when_collecting", () => {
    const { status } = read();
    const line = strip("healthy", status, SPEND_NOW);
    expect(line.tone).toBe("ok");
    expect(line.detail).toContain("Receiver on 127.0.0.1:4318");
    expect(line.detail).toContain("last event 14 s ago");
    expect(line.detail).toContain("18.4 of 64 MB");
    expect(line.detail).toContain("since 11-02");
  });

  it("test08_the_error_strip_names_the_cause_and_offers_another_port", () => {
    const { status } = read({ scenario: "cannot-listen" });
    expect(strip("receiverCannotListen", status, SPEND_NOW)).toMatchObject({
      tone: "danger",
      detail: "Port 4318 is used by another program.",
      action: "port",
    });
  });

  it("test09_computing_shows_the_file_count_and_how_long_it_has_taken", () => {
    const { status } = read({ scenario: "computing" });
    const bar = progress(status, 14_000);
    expect(bar.detail).toContain("77 of 461 files");
    expect(bar.detail).toContain("14 s");
    expect(bar.fraction).toBeCloseTo(77 / 461);
  });

  it("test10_not_exporting_is_flagged_with_the_causes_found", () => {
    const { status, summary } = read({ stale: true });
    const [notice] = notices(summary, status, SPEND_NOW);
    expect(notice).toMatchObject({ id: "not-exporting", tone: "warning" });
    expect(notice.detail).toContain("12 sessions");
    expect(staleCauses(summary, status)).toEqual([
      "receiver is listening",
      "~/work/clients/alpha/api/.claude/settings.json sets OTEL_EXPORTER_OTLP_ENDPOINT to another address",
      "3 sessions started before setup (restart them)",
    ]);
  });

  it("test11_drops_skipped_lines_and_a_full_store_are_never_silent", () => {
    const { status, summary } = read();
    expect(notices(summary, status, SPEND_NOW)).toEqual([]);
    const worse: StatusData = {
      ...status,
      receiver: { ...status.receiver, dropped: 3 },
      store: { ...status.store, skippedLines: 3, bytes: status.store.maxBytes },
    };
    expect(notices(summary, worse, SPEND_NOW).map((n) => n.id)).toEqual([
      "dropped",
      "skipped",
      "size",
    ]);
  });
});

describe("range tiles", () => {
  it("test12_each_tile_carries_cost_delta_and_the_transcript_share", () => {
    const { summary } = read();
    const [today, week, month] = tiles(summary, true);
    expect(today.headline).toBe("$31.40");
    expect(today.detail).toBe("108.6M tokens · so far");
    expect(today.notes).toEqual(["Yesterday $56.20 in total"]);
    expect(week.headline).toBe("$184.20");
    expect(week.notes[0]).toMatch(/^[+-]\d+% vs the 7 days before$/);
    expect(week.notes[1]).toBe("2 of 7 days from transcripts (estimate)");
    expect(month.label).toBe("30 days");
  });

  it("test13_transcripts_only_numbers_are_badged_estimate", () => {
    const { summary } = read({ scenario: "off" });
    const [, week] = tiles(summary, true);
    expect(week.badge).toBe("Estimate");
    expect(week.notes).toContain("from transcripts only (estimate)");
  });

  it("test14_with_cost_switched_off_the_headline_is_tokens", () => {
    const { summary } = read();
    const [today, week] = tiles(summary, false);
    expect(today.headline).toBe("108.6M tokens");
    expect(week.notes.join(" ")).not.toContain("$");
    expect(today.notes).toEqual([]);
  });
});

describe("cost by day", () => {
  it("test15_one_bar_per_day_with_the_closed_app_days_hatched", () => {
    const { summary } = read();
    const chart = bars(summary, SPEND_NOW);
    expect(chart.map((b) => b.monthDay)).toEqual([
      "11-12",
      "11-13",
      "11-14",
      "11-15",
      "11-16",
      "11-17",
      "11-18",
    ]);
    expect(chart.map((b) => b.label)).toEqual([
      "$19.8",
      "$27.1",
      "$6.9 est.",
      "$4.2 est.",
      "$38.6",
      "$56.2",
      "$31.4",
    ]);
    expect(chart[2].hatched).toBe(1);
    expect(chart[6].hatched).toBe(0);
    expect(chart[6].soFar).toBe(true);
    expect(Math.max(...chart.map((b) => b.height))).toBe(1);
  });

  it("test16_bars_stack_by_model_family_and_the_legend_lists_what_appears", () => {
    const { summary } = read();
    const chart = bars(summary, SPEND_NOW);
    expect(chart[5].segments.map((s) => s.family)).toEqual(["Opus", "Sonnet", "Haiku"]);
    const sum = chart[5].segments.reduce((total, s) => total + s.usd, 0);
    expect(sum).toBeCloseTo(chart[5].usd, 2);
    expect(legend(chart)).toEqual(["Opus", "Sonnet", "Haiku"]);
  });

  it("test17_a_day_without_events_is_an_empty_bar_not_a_gap", () => {
    const { summary } = read({}, "--range", "7d");
    const trimmed: SummaryData = { ...summary, byDay: summary.byDay.slice(-2) };
    const chart = bars(trimmed, SPEND_NOW);
    expect(chart).toHaveLength(7);
    expect(chart[0]).toMatchObject({ usd: 0, height: 0, label: "$0.00" });
  });

  it("test18_the_table_view_says_how_each_day_was_counted", () => {
    const { summary } = read();
    const table = chartTable(bars(summary, SPEND_NOW));
    expect(table[2]).toMatchObject({ day: "Sat 11-14", basis: "Estimate" });
    expect(table[0].basis).toBe("Measured");
    expect(table[6].day).toBe("Wed 11-18 (so far)");
  });

  it("test19_today_is_one_bar_and_thirty_days_are_thirty", () => {
    expect(bars(read({}, "--range", "today").summary, SPEND_NOW)).toHaveLength(1);
    expect(bars(read({}, "--range", "30d").summary, SPEND_NOW)).toHaveLength(30);
  });
});

describe("cache and breakdowns", () => {
  it("test20_cache_lines_split_all_tokens_into_four_shares", () => {
    const { summary } = read();
    const lines = cacheLines(summary.totals.tokens);
    expect(lines.map((l) => l.label)).toEqual([
      "Cache read",
      "Cache write",
      "Fresh input",
      "Output",
    ]);
    expect(lines[0].share).toBe("95.8%");
  });

  it("test21_project_rows_are_filterable_and_the_rest_rolls_up", () => {
    const { summary } = read();
    const rows = breakdown("project", summary.byProject);
    expect(rows).toHaveLength(6);
    expect(rows[0]).toMatchObject({
      label: "~/work/clients/alpha/web-app",
      filterable: true,
      sub: "41 sessions",
    });
    expect(rows[5]).toMatchObject({
      label: "3 other folders",
      rollup: true,
      filterable: false,
    });
  });

  it("test22_model_rows_show_the_family_and_the_model_name", () => {
    const { summary } = read();
    const [opus] = breakdown("model", summary.byModel);
    expect(opus).toMatchObject({ label: "Opus", share: "64%" });
    expect(opus.sub).toBe("claude-opus-5 · 3,120 requests");
  });

  it("test23_task_rows_name_interactive_apart_from_unattributed", () => {
    const rows: SpendRow[] = read().summary.byTask;
    const shown = breakdown("task", rows);
    expect(shown[0]).toMatchObject({
      label: "Interactive (no task)",
      key: "Interactive",
    });
    const last = shown.find((r) => r.key === "Unattributed");
    expect(last?.sub).toBe("history without a session id");
    expect(shown.every((r) => r.badge === "Estimate" || r.badge === "Measured")).toBe(
      true,
    );
    expect(shown.some((r) => r.filterable)).toBe(false);
  });

  it("test24_a_row_with_any_list_price_cost_is_badged_estimate", () => {
    const { summary } = read({ scenario: "off" });
    expect(breakdown("model", summary.byModel).every((r) => r.badge === "Estimate")).toBe(
      true,
    );
  });
});

describe("filter", () => {
  it("test25_enter_on_the_same_row_again_clears_the_filter", () => {
    const on = toggleProject(NO_FILTER, PROJECT_WEB);
    expect(on).toEqual({ project: PROJECT_WEB });
    expect(toggleProject(on, PROJECT_WEB)).toEqual(NO_FILTER);
    expect(toggleProject(on, "/other")).toEqual({ project: "/other" });
  });

  it("test26_the_filter_note_counts_sessions_and_share_of_the_whole", () => {
    const whole = read().summary;
    const one = read({}, "--project", PROJECT_WEB).summary;
    expect(filterNote(whole, null)).toBeNull();
    const note = filterNote(one, whole) ?? "";
    expect(note).toMatch(/^Filtered\. \d+ of \d+ sessions, 39% of the spend\.$/);
    expect(filterNote(one, null)).toMatch(/^Filtered\. \d+ sessions\.$/);
  });

  it("test27_the_session_list_is_labelled_by_source_and_start", () => {
    const one = read({}, "--project", PROJECT_WEB).summary;
    const [first] = sessionLines(one.sessionList ?? []);
    expect(first).toMatchObject({ source: "OTel", cost: "$14.2", tokens: "49.1M" });
    expect(first.started).toContain("17");
  });
});

describe("the MCP card", () => {
  it("test28_tool_definitions_compare_exposed_with_every_tool_listed", () => {
    const { summary } = read();
    const card = mcpCard(summary.mcpPart, summary.totals.requests);
    expect(card.toolDefinitions).toMatchObject({
      exposed: "4.1k",
      full: "61.3k",
      sessions: 142,
      saved: "8.12M",
    });
    expect(card.share?.text).toBe("≈ 6%");
    expect(card.share?.badge).toBe("Estimate");
    expect(card.compression).toMatchObject({
      cut: "1.31M",
      calls: 2154,
      window: "11-05 to 11-18",
    });
  });

  it("test29_the_first_request_folds_skills_into_the_part_that_is_not_itemised", () => {
    const { summary } = read();
    const request = mcpCard(summary.mcpPart, 1).firstRequest;
    expect(request?.total).toBe("31,240");
    expect(request?.folder).toBe("~/work/clients/alpha/web-app");
    const [instructions, tools, rest] = request?.segments ?? [];
    expect([instructions.tokens, tools.tokens, rest.tokens]).toEqual([
      7910, 4100, 19_230,
    ]);
    expect(rest.badge).toBe("Not itemised");
    expect(tools.badge).toBe("Counted");
    expect(instructions.badge).toBe("Estimate");
    const total = request?.segments.reduce((sum, s) => sum + s.fraction, 0);
    expect(total).toBeCloseTo(1);
  });

  it("test30_a_failed_part_fails_alone_with_its_cause", () => {
    const { summary } = read({ mcpError: true });
    const card = mcpCard(summary.mcpPart, 1);
    expect(card.failed).toBe("savings-v4.jsonl: line 812 is not valid JSON.");
    expect(card.toolDefinitions).toBeNull();
    expect(tiles(summary, true)).toHaveLength(3);
  });

  it("test31_a_first_request_without_an_instruction_estimate_has_two_segments", () => {
    const { summary } = read();
    if (summary.mcpPart.state === "error" || !summary.mcpPart.firstRequest)
      throw new Error("fixture");
    const part = {
      ...summary.mcpPart,
      firstRequest: { ...summary.mcpPart.firstRequest, instructionTokens: null },
    };
    const request = mcpCard(part, 1).firstRequest;
    expect(request?.segments.map((s) => s.id)).toEqual(["tools", "rest"]);
  });
});

describe("the collecting card", () => {
  it("test32_it_says_where_collecting_stands_and_what_was_written", () => {
    const { status } = read();
    const card = collectingCard(status, SPEND_NOW);
    expect(card.receiver).toBe(
      "Listening on 127.0.0.1:4318 (this computer only) · started with the Toolport app",
    );
    expect(card.stored).toBe("18.4 of 64 MB · 41,206 events");
    expect(card.keptFor).toBe("16 of 30 days · oldest 11-02");
    expect(card.claudeCode).toBe(
      "5 settings written in ~/.claude/settings.json on 11-02",
    );
  });

  it("test33_without_setup_it_says_nothing_was_written", () => {
    const { status } = read({ scenario: "never" });
    const card = collectingCard(status, SPEND_NOW);
    expect(card.on).toBe(false);
    expect(card.stored).toBe("No spend data on this computer.");
    expect(card.claudeCode).toBe("No OpenTelemetry settings written by Toolport.");
    expect(card.receiver).toBe("Not running.");
  });
});
