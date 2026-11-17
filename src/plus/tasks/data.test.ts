import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type {
  BusyData,
  EnableData,
  RunPlanData,
  RunView,
  SavePlanData,
  TaskHistoryData,
  TaskListData,
  TaskMissedData,
  TaskShowData,
} from "./data";

const ctl = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

function golden<T>(name: string): { ok: boolean; data?: T; error?: unknown } {
  return JSON.parse(readFileSync(join(ctl, `${name}.json`), "utf8")).envelope;
}

const TRIGGERS = ["manual", "cli", "selfMcp", "schedule", "catch-up", "onAuthFailure"];
const STATUSES = [
  "pending",
  "running",
  "waiting",
  "ok",
  "failed",
  "cancelled",
  "skipped",
];

function expectRun(run: RunView) {
  expect(TRIGGERS).toContain(run.trigger);
  expect(STATUSES).toContain(run.status);
  expect(typeof run.id).toBe("string");
  expect(Array.isArray(run.steps)).toBe(true);
  if (run.trigger === "catch-up") {
    expect(run.missed?.slots).toBeGreaterThan(0);
    expect(typeof run.missed?.firstDue).toBe("string");
  }
}

describe("task envelopes", () => {
  it("test01_every_task_ls_golden_matches_the_list_shape", () => {
    const names = readdirSync(ctl).filter((f) => f.startsWith("task-ls."));
    expect(names.length).toBeGreaterThan(2);
    for (const file of names) {
      const { ok, data } = golden<TaskListData>(file.replace(/\.json$/, ""));
      if (!ok) continue;
      expect(Array.isArray(data?.tasks), file).toBe(true);
      for (const row of data!.tasks) {
        expect(typeof row.id).toBe("string");
        expect(typeof row.enabled).toBe("boolean");
        expect(typeof row.waiting).toBe("boolean");
        expect(["login", "script"]).toContain(row.kind);
        expect(row.triggers.selfMcp.approval).toBe("every-run");
        if (row.lastRun) expect(STATUSES).toContain(row.lastRun.status);
        for (const problem of row.problems ?? [])
          expect(problem.code).toBe("routine-step");
      }
    }
  });

  it("test02_the_routine_task_is_listed_with_its_problem_and_shown_as_stored", () => {
    const { data } = golden<TaskListData>("task-ls.routine-and-scheduler");
    const row = data!.tasks.find((t) => t.id === "refresh-reports");
    expect(row?.problems?.[0]).toMatchObject({
      code: "routine-step",
      step: "refresh",
      index: 1,
    });
    expect(data!.invalid.map((i) => i.id)).toEqual(["broken"]);
    expect(data!.scheduler).toMatchObject({ by: "app", intervalSecs: 20 });
    const shown = golden<TaskShowData>("task-show.routine").data!;
    const step = shown.task.steps[1];
    expect("routineId" in step && step.routineId).toBe("reports-refresh");
    expect(shown.problems?.[0].routine).toBe("reports-refresh");
  });

  it("test03_a_catch_up_run_names_what_it_covers", () => {
    const { data } = golden<TaskHistoryData>("task-history.catch-up");
    data!.runs.forEach(expectRun);
    expect(data!.runs.some((r) => r.trigger === "catch-up")).toBe(true);
  });

  it("test04_the_ledger_comes_back_with_all_three_outcomes", () => {
    const { data } = golden<TaskMissedData>("task-missed.all");
    expect(data!.missed.map((m) => m.outcome).sort()).toEqual([
      "busy",
      "caught-up",
      "too-old",
    ]);
    expect(data!.missed.find((m) => m.outcome === "caught-up")?.run).toBe(
      "run-fixture-catchup",
    );
    expect(golden<TaskMissedData>("task-missed.none").data!.missed).toEqual([]);
  });

  it("test05_a_refused_second_start_carries_the_live_run", () => {
    const envelope = golden<unknown>("task-run.busy") as { ok: boolean; data?: BusyData };
    expect(envelope.ok).toBe(false);
    expect(envelope.data).toMatchObject({
      run: "run-fixture-waiting",
      task: "portal-token",
    });
  });

  it("test06_a_dry_run_of_edit_lists_the_next_five_runs", () => {
    const { data } = golden<SavePlanData>("task-edit.preview");
    expect(data!.dryRun).toBe(true);
    expect(data!.nextRuns).toHaveLength(5);
    expect(golden<SavePlanData>("task-add.preview").data!.nextRuns).toBeUndefined();
  });

  it("test07_enable_and_disable_answer_with_a_plan_and_a_result", () => {
    const preview = golden<EnableData>("task-enable.preview").data!;
    expect(preview).toMatchObject({
      dryRun: true,
      enabled: true,
      task: "draft-cleanup",
      result: null,
    });
    expect(golden<EnableData>("task-enable.apply").data!.result?.applied).toBe(true);
    expect(golden<EnableData>("task-enable.again").data!.result?.applied).toBe(false);
    expect(golden<EnableData>("task-disable.apply").data!.enabled).toBe(false);
  });

  it("test08_a_run_plan_and_the_run_views_keep_their_shape", () => {
    const preview = golden<RunPlanData>("task-run.preview").data!;
    expect(preview.run).toBeNull();
    expect(preview.plan.steps.length).toBeGreaterThan(0);
    golden<TaskShowData>("task-show.waiting").data!.runs.forEach(expectRun);
  });
});
