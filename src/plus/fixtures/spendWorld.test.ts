import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { SetupPlanData, PruneData, StatusData, SummaryData } from "../spend/data";
import { createSpendWorld, PROFILE_WORK, PROJECT_WEB } from "./spendWorld";

const dir = join(__dirname, "../../../plus/fixtures/ctl-envelopes");
const golden = (name: string) =>
  JSON.parse(readFileSync(join(dir, `${name}.json`), "utf8")).envelope.data;
const keys = (value: unknown) => Object.keys(value as object).sort();

const run = <T>(world: ReturnType<typeof createSpendWorld>, ...argv: string[]) => {
  const reply = world.run(["spend", ...argv]);
  if (!reply.ok) throw new Error(reply.error?.message);
  return reply.data as T;
};

describe("the spend fixture world", () => {
  it("test01_its_answers_have_the_keys_of_the_rust_goldens", () => {
    const world = createSpendWorld();
    expect(keys(run<StatusData>(world, "status"))).toEqual(
      keys(golden("spend-status.on")),
    );
    const summary = run<SummaryData>(world, "summary");
    expect(keys(summary).filter((k) => k !== "sessionList")).toEqual(
      keys(golden("spend-summary.default")),
    );
    expect(keys(summary.byDay[0])).toEqual(
      keys(golden("spend-summary.default").byDay[0]),
    );
    expect(keys(summary.tiles["7d"])).toEqual(
      keys(golden("spend-summary.default").tiles["7d"]),
    );
    expect(keys(run<PruneData>(world, "prune", "--dry-run"))).toEqual(
      keys(golden("spend-prune.dry-run")),
    );
    const plan = run<SetupPlanData>(world, "setup", "plan");
    expect(keys(plan)).toEqual(keys(golden("spend-setup-plan.conflict")));
    expect(keys(plan.profiles[0])).toEqual(
      keys(golden("spend-setup-plan.conflict").profiles[0]),
    );
  });

  it("test02_each_scenario_starts_in_its_state", () => {
    const state = (scenario: Parameters<typeof createSpendWorld>[0]) =>
      run<StatusData>(createSpendWorld(scenario), "status");
    expect(state({ scenario: "never" })).toMatchObject({
      state: "off",
      transcripts: { files: 0 },
    });
    expect(state({ scenario: "off" }).transcripts.files).toBeGreaterThan(0);
    expect(state({ scenario: "waiting" })).toMatchObject({ state: "waiting" });
    expect(state({ scenario: "cannot-listen" }).receiver.error).toMatch(/4318/);
    const computing = state({ scenario: "computing" });
    expect(computing.transcripts.done).toBeLessThan(computing.transcripts.files);
  });

  it("test03_transcript_days_are_hatched_and_a_project_filter_narrows_the_report", () => {
    const world = createSpendWorld();
    const week = run<SummaryData>(world, "summary", "--range", "7d");
    const closed = week.byDay.find((d) => d.key === "2026-11-14");
    expect(closed?.cost.estimatedUsd).toBe(closed?.cost.usd);
    expect(week.byDay.find((d) => d.key === "2026-11-18")?.cost.estimatedUsd).toBe(0);
    const one = run<SummaryData>(world, "summary", "--project", PROJECT_WEB);
    expect(one.totals.cost.usd).toBeLessThan(week.totals.cost.usd);
    expect(one.sessionList?.length).toBeGreaterThan(0);
    expect(one.byProject.map((p) => p.key)).toEqual([PROJECT_WEB]);
  });

  it("test04_setup_apply_moves_a_never_set_up_world_to_waiting_and_undo_back", () => {
    const world = createSpendWorld({ scenario: "off" });
    run(world, "setup", "apply");
    expect(run<StatusData>(world, "status")).toMatchObject({
      state: "waiting",
      profilesSetUp: 2,
    });
    run(world, "setup", "undo");
    expect(run<StatusData>(world, "status")).toMatchObject({
      state: "off",
      profilesSetUp: 0,
    });
  });

  it("test05_a_chosen_profile_alone_is_written_and_an_unknown_one_is_refused", () => {
    const world = createSpendWorld({ scenario: "off", conflict: true });
    const plan = run<SetupPlanData>(world, "setup", "plan", "--profile", PROFILE_WORK);
    expect(plan.profiles.map((p) => p.profile)).toEqual([PROFILE_WORK]);
    expect(world.run(["spend", "setup", "plan", "--profile", "/nowhere"]).ok).toBe(false);
    const conflict = run<SetupPlanData>(world, "setup", "plan");
    expect(conflict.conflicts).toBe(1);
    expect(
      run<SetupPlanData>(world, "setup", "plan", "--overwrite", "OTEL_METRICS_EXPORTER")
        .conflicts,
    ).toBe(0);
  });

  it("test06_deleting_data_needs_yes_and_every_profile_undone", () => {
    const world = createSpendWorld();
    run(world, "setup", "apply");
    const refused = world.run(["spend", "setup", "undo", "--delete-data"]);
    expect(refused.error?.message).toMatch(/--yes/);
    const partial = world.run([
      "spend",
      "setup",
      "undo",
      "--profile",
      PROFILE_WORK,
      "--delete-data",
      "--yes",
    ]);
    expect(partial.error?.message).toMatch(/every profile/);
    run(world, "setup", "undo", "--delete-data", "--yes");
    expect(run<StatusData>(world, "status").store.events).toBe(0);
  });

  it("test07_config_set_refuses_values_outside_the_bounds_and_reports_a_port_change", () => {
    const world = createSpendWorld();
    expect(world.run(["spend", "config", "set", "--retention-days", "3"]).ok).toBe(false);
    expect(world.run(["spend", "config", "set", "--max-mb", "999"]).ok).toBe(false);
    expect(
      run<{ portChanged: boolean }>(world, "config", "set", "--port", "4999").portChanged,
    ).toBe(true);
    expect(world.port).toBe(4999);
  });
});
