import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: { argv: string[]; signal?: AbortSignal }[] = [];

vi.mock("../bridge/ctl", () => ({
  ctlData: vi.fn(async (argv: string[], options?: { signal?: AbortSignal }) => {
    calls.push({ argv, signal: options?.signal });
    return {};
  }),
}));

import {
  deleteTotals,
  exportCsv,
  pruneNow,
  prunePreview,
  setConfig,
  setupApply,
  setupApplyDryRun,
  setupPlan,
  setupUndo,
  setupUndoPreview,
  spendStatus,
  spendSummary,
} from "./commands";

const last = () => calls[calls.length - 1].argv;

beforeEach(() => {
  calls.length = 0;
});

describe("spend commands", () => {
  it("test01_the_report_asks_for_a_range_and_optionally_one_folder", async () => {
    await spendSummary("30d");
    expect(last()).toEqual(["spend", "summary", "--range", "30d"]);
    await spendSummary("today", { project: "/work/alpha" });
    expect(last()).toEqual([
      "spend",
      "summary",
      "--range",
      "today",
      "--project",
      "/work/alpha",
    ]);
  });

  it("test01b_model_and_task_filters_are_passed_beside_the_folder", async () => {
    await spendSummary("7d", { model: "claude-opus-5", task: "Interactive" });
    expect(last()).toEqual([
      "spend",
      "summary",
      "--range",
      "7d",
      "--model",
      "claude-opus-5",
      "--task",
      "Interactive",
    ]);
  });

  it("test02_the_abort_signal_reaches_the_bridge", async () => {
    const abort = new AbortController();
    await spendSummary("7d", {}, abort.signal);
    await spendStatus(abort.signal);
    expect(calls.map((c) => c.signal)).toEqual([abort.signal, abort.signal]);
  });

  it("test03_a_plan_and_its_apply_take_the_same_choices", async () => {
    const choice = {
      port: 4999,
      overwrite: ["OTEL_METRICS_EXPORTER"],
      profiles: ["/a/.claude", "/b/.claude"],
    };
    const tail = [
      "--port",
      "4999",
      "--overwrite",
      "OTEL_METRICS_EXPORTER",
      "--profile",
      "/a/.claude",
      "--profile",
      "/b/.claude",
    ];
    await setupPlan(choice);
    expect(last()).toEqual(["spend", "setup", "plan", ...tail]);
    await setupApply(choice);
    expect(last()).toEqual(["spend", "setup", "apply", ...tail]);
    await setupApplyDryRun({ port: 4999 });
    expect(last()).toEqual(["spend", "setup", "apply", "--dry-run", "--port", "4999"]);
    await setupPlan();
    expect(last()).toEqual(["spend", "setup", "plan"]);
  });

  it("test04_deleting_the_data_is_previewed_first_and_confirmed_with_yes", async () => {
    await setupUndoPreview({ deleteData: true });
    expect(last()).toEqual(["spend", "setup", "undo", "--dry-run", "--delete-data"]);
    await setupUndo({ deleteData: true });
    expect(last()).toEqual(["spend", "setup", "undo", "--delete-data", "--yes"]);
  });

  it("test05_undo_keeps_the_data_unless_asked_and_never_adds_yes_for_it", async () => {
    await setupUndo();
    expect(last()).toEqual(["spend", "setup", "undo"]);
    await setupUndo({ profiles: ["/a/.claude"] });
    expect(last()).toEqual(["spend", "setup", "undo", "--profile", "/a/.claude"]);
  });

  it("test06_prune_previews_runs_and_deletes_totals_only_with_yes", async () => {
    await prunePreview();
    expect(last()).toEqual(["spend", "prune", "--dry-run"]);
    await pruneNow();
    expect(last()).toEqual(["spend", "prune"]);
    await deleteTotals();
    expect(last()).toEqual(["spend", "prune", "--totals", "--yes"]);
  });

  it("test07_config_sends_only_what_changed", async () => {
    await setConfig({ retentionDays: 14, showCost: false });
    expect(last()).toEqual([
      "spend",
      "config",
      "set",
      "--retention-days",
      "14",
      "--show-cost",
      "off",
    ]);
    await setConfig({ port: 4999, maxMb: 32, showCost: true });
    expect(last()).toEqual([
      "spend",
      "config",
      "set",
      "--port",
      "4999",
      "--max-mb",
      "32",
      "--show-cost",
      "on",
    ]);
  });

  it("test08_export_prints_without_a_file_and_writes_with_one", async () => {
    await exportCsv("30d");
    expect(last()).toEqual(["spend", "export-csv", "--range", "30d"]);
    await exportCsv("7d", "/tmp/spend.csv");
    expect(last()).toEqual([
      "spend",
      "export-csv",
      "--range",
      "7d",
      "--out",
      "/tmp/spend.csv",
    ]);
  });
});
