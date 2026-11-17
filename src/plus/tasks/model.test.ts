import { describe, expect, it } from "vitest";
import type { MissedRecord, RunView, TaskRow } from "./data";
import {
  cronOf,
  describeCron,
  duration,
  elapsed,
  presetOf,
  rowState,
  selfStarting,
  slugify,
  sortRows,
  timeline,
} from "./model";

const NOW = Date.parse("2026-11-17T08:58:00Z");

function row(id: string, extra: Partial<TaskRow> = {}): TaskRow {
  return {
    id,
    title: id,
    enabled: true,
    triggers: {
      manual: true,
      cli: false,
      selfMcp: { enabled: false, approval: "every-run" },
      schedule: null,
      onAuthFailure: [],
    },
    lastRun: null,
    nextRun: null,
    refreshes: [],
    waiting: false,
    kind: "script",
    ...extra,
  };
}

const run = (id: string, startedAt: string): RunView => ({
  id,
  task: "a",
  trigger: "schedule",
  status: "ok",
  startedAt,
  endedAt: null,
  durationMs: null,
  error: null,
  steps: [],
});

describe("task model", () => {
  it("test01_cron_wording_names_hours_minutes_and_falls_back_to_the_cron", () => {
    expect(describeCron("7 1-23/4 * * *", null)).toBe("Every 4 hours at :07");
    expect(describeCron("7 */6 * * *", null)).toBe("Every 6 hours at :07");
    expect(describeCron("*/15 * * * *", null)).toBe("Every 15 minutes");
    expect(describeCron("0 7 * * *", null)).toBe("Every day at 07:00 UTC");
    expect(describeCron("0 7 1 * *", null)).toBeNull();
    expect(describeCron("nonsense", null)).toBeNull();
  });

  it("test02_a_daily_or_weekly_slot_reads_in_local_time_from_the_next_run", () => {
    const next = Date.parse("2026-11-23T07:00:00Z");
    const text = describeCron("0 7 * * 1", next) ?? "";
    expect(text).toMatch(/^Mondays at \d\d:\d\d$/);
  });

  it("test03_the_presets_round_trip_through_their_cron", () => {
    for (const cron of ["7 */4 * * *", "30 3 * * *", "0 8 * * 1"]) {
      const { preset, fields } = presetOf(cron);
      expect(cronOf(preset, fields, "")).toBe(cron);
    }
    expect(presetOf("1,2 * * * *").preset).toBe("custom");
  });

  it("test04_rows_sort_needs_you_then_failed_then_next_run_with_off_last", () => {
    const rows = [
      row("off", { enabled: false }),
      row("later", {
        nextRun: "2026-11-20T08:00:00Z",
        triggers: {
          ...row("x").triggers,
          schedule: { cron: "0 8 * * *", autoRun: true },
        },
      }),
      row("soon", {
        nextRun: "2026-11-17T10:07:00Z",
        triggers: {
          ...row("x").triggers,
          schedule: { cron: "7 * * * *", autoRun: true },
        },
      }),
      row("failed", {
        lastRun: {
          runId: "r",
          status: "failed",
          startedAt: "2026-11-17T05:00:00Z",
          durationMs: 1,
        },
      }),
      row("waits", { waiting: true }),
    ];
    expect(sortRows(rows).map((r) => r.id)).toEqual([
      "waits",
      "failed",
      "soon",
      "later",
      "off",
    ]);
  });

  it("test05_a_routine_problem_beats_the_other_states", () => {
    const broken = row("b", {
      problems: [
        { code: "routine-step", step: "s", index: 0, routine: "r", message: "m" },
      ],
    });
    expect(rowState(broken).key).toBe("cannot-run");
    expect(rowState(row("w", { waiting: true })).label).toBe("Needs you");
    expect(rowState(row("o", { enabled: false })).label).toBe("Off");
  });

  it("test06_only_enabled_auto_run_schedules_start_on_their_own", () => {
    const auto = row("a", {
      triggers: { ...row("x").triggers, schedule: { cron: "7 * * * *", autoRun: true } },
    });
    const asks = row("b", {
      triggers: { ...row("x").triggers, schedule: { cron: "7 * * * *", autoRun: false } },
    });
    const off = row("c", { enabled: false, triggers: auto.triggers });
    expect(selfStarting([auto, asks, off]).map((r) => r.id)).toEqual(["a"]);
  });

  it("test07_the_timeline_puts_skipped_slots_between_runs_and_hides_caught_up_ones", () => {
    const missed: MissedRecord[] = [
      {
        task: "a",
        slots: 1,
        firstDue: "2026-11-16T02:07:00Z",
        lastDue: "2026-11-16T02:07:00Z",
        outcome: "busy",
        at: "2026-11-16T02:07:00Z",
      },
      {
        task: "a",
        slots: 2,
        firstDue: "2026-11-16T13:07:00Z",
        lastDue: "2026-11-16T17:07:00Z",
        outcome: "caught-up",
        at: "2026-11-16T20:00:00Z",
      },
    ];
    const runs = [run("new", "2026-11-17T06:07:00Z"), run("old", "2026-11-16T01:00:00Z")];
    expect(
      timeline(runs, missed, true).map((i) =>
        i.kind === "run" ? i.run.id : i.record.outcome,
      ),
    ).toEqual(["new", "busy", "old"]);
    expect(timeline(runs, missed, false)).toHaveLength(2);
    expect(NOW).toBeGreaterThan(0);
  });

  it("test08_small_formatters", () => {
    expect(duration(41_000)).toBe("41 s");
    expect(duration(252_000)).toBe("4 min 12 s");
    expect(duration(180_000)).toBe("3 min");
    expect(elapsed(41)).toBe("0:41");
    expect(slugify("Sync the notes mirror!")).toBe("sync-the-notes-mirror");
  });
});
