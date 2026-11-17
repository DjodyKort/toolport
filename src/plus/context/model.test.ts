import { describe, expect, it } from "vitest";
import type { FolderStatusData, LoadItem } from "./data";
import { headlineOf, layerOf, leversOf, markOf, splitStack } from "./model";

const CWD = "/work/app";

function item(extra: Partial<LoadItem>): LoadItem {
  return {
    kind: "memory",
    name: "CLAUDE.md",
    path: `${CWD}/CLAUDE.md`,
    source: "project",
    loaded: true,
    lazy: false,
    reason: "",
    tokens: 100,
    leftOutBy: null,
    ...extra,
  };
}

function status(extra: Partial<FolderStatusData> = {}): FolderStatusData {
  return {
    cwd: CWD,
    bundle: { folder: CWD, applied: null, conflicts: [] },
    leftOut: { files: [], memoryOff: false },
    pluginsOff: [],
    estimate: { totalTokens: 500, basis: "estimate", tokenizer: "cl100k_base" },
    measured: null,
    calibration: null,
    ...extra,
  };
}

const measured = (stale: boolean) => ({
  total: 2000,
  windowPercent: 1,
  model: "haiku",
  claudeCodeVersion: "2.1.295",
  measuredAt: "2026-11-16T10:00:00Z",
  stale,
});
const calibration = (estimated: number) => ({
  factor: 4,
  measured: 2000,
  estimated,
  measuredAt: "2026-11-16T10:00:00Z",
  stale: false,
  calibratedTotal: 0,
  meaning: "",
});

describe("context model", () => {
  it("test01_the_headline_is_an_estimate_until_a_measurement_exists", () => {
    expect(headlineOf(status(), null)).toEqual({ kind: "estimate", total: 500 });
  });

  it("test02_a_fresh_measurement_keeps_its_number_and_names_the_not_itemised_part", () => {
    const state = headlineOf(
      status({ measured: measured(false), calibration: calibration(500) }),
      null,
    );
    expect(state).toMatchObject({ kind: "measured", total: 2000, notItemised: 1500 });
  });

  it("test03_a_change_since_the_measurement_projects_the_total_by_the_estimated_saving", () => {
    const state = headlineOf(
      status({
        measured: measured(true),
        calibration: calibration(800),
        estimate: { totalTokens: 500, basis: "estimate", tokenizer: "cl100k_base" },
      }),
      null,
    );
    expect(state).toMatchObject({ kind: "projected", total: 1700 });
  });

  it("test04_a_new_claude_code_version_alone_leaves_the_number_stale_not_projected", () => {
    const state = headlineOf(
      status({ measured: measured(true), calibration: calibration(500) }),
      null,
    );
    expect(state).toMatchObject({ kind: "stale", total: 2000 });
  });

  it("test05_levers_are_the_biggest_loaded_instruction_files_and_never_the_memory_index", () => {
    const rows = [
      item({ path: "/a", tokens: 10 }),
      item({ path: "/b", tokens: 90 }),
      item({ kind: "memory-index", path: "/m", tokens: 500 }),
      item({ path: "/c", tokens: 50, leftOutBy: "toolport", loaded: false }),
    ];
    expect(leversOf(rows).map((l) => l.item.path)).toEqual(["/b", "/a"]);
  });

  it("test06_rows_belong_to_a_layer_by_source_and_place", () => {
    expect(layerOf(item({ source: "policy" }), CWD)).toBe("Org");
    expect(layerOf(item({ source: "user" }), CWD)).toBe("You");
    expect(layerOf(item({ source: "local" }), CWD)).toBe("Local");
    expect(layerOf(item({}), CWD)).toBe("This folder");
    expect(layerOf(item({ path: "/work/CLAUDE.md" }), CWD)).toBe("Parent folder");
  });

  it("test07_a_row_is_marked_from_the_log_and_a_mismatch_wins", () => {
    const verified = {
      status: "mismatch",
      checked: 2,
      loaded: 1,
      absent: 0,
      mismatches: [{ path: "/x", expected: "loaded", actual: "absent" }],
      unknown: [],
      measuredAt: "",
      claudeCodeVersion: "",
      stale: false,
    };
    expect(markOf(item({ path: "/x" }), verified)).toBe("mismatch");
    expect(markOf(item({ path: "/y" }), verified)).toBe("loaded");
    expect(markOf(item({ path: "/y", loaded: false }), verified)).toBe("absent");
    expect(markOf(item({ path: "/y" }), null)).toBeNull();
  });

  it("test08_the_stack_splits_into_start_lazy_left_out_and_not_here", () => {
    const parts = splitStack([
      item({ path: "/1" }),
      item({ path: "/2", lazy: true, loaded: false }),
      item({ path: "/3", loaded: false, leftOutBy: "toolport" }),
      item({ path: "/4", loaded: false }),
      item({ kind: "memory-index", path: "/5" }),
    ]);
    expect(parts.start).toHaveLength(1);
    expect(parts.lazy).toHaveLength(1);
    expect(parts.leftOut).toHaveLength(1);
    expect(parts.notHere).toHaveLength(1);
    expect(parts.memory).toHaveLength(1);
  });
});
