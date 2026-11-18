import { describe, expect, it } from "vitest";
import add from "../../../plus/fixtures/ctl-envelopes/client-direct-add.plan.json";
import applied from "../../../plus/fixtures/ctl-envelopes/client-direct-add.apply.json";
import removed from "../../../plus/fixtures/ctl-envelopes/client-direct-rm.plan.json";
import forgotten from "../../../plus/fixtures/ctl-envelopes/client-direct-rm.forgotten.json";
import list from "../../../plus/fixtures/ctl-envelopes/client-direct-ls.entries.json";
import type { DirectListData, DirectOutcome } from "./data";

const outcome = (golden: { envelope: { data: unknown } }) =>
  golden.envelope.data as DirectOutcome;

describe("the golden envelopes of client direct", () => {
  it("test01_a_plan_names_its_hash_the_change_and_the_undo", () => {
    const plan = outcome(add);
    expect(plan.dryRun).toBe(true);
    expect(plan.result).toBeNull();
    expect(plan.planHash).toBeTruthy();
    expect(plan.plan.steps.some((step) => step.diff)).toBe(true);
    expect(plan.plan.undo).toContain("client direct rm");
  });

  it("test02_an_applied_add_reports_the_backup_and_what_it_changed", () => {
    const done = outcome(applied);
    expect(done.result).toMatchObject({ applied: true });
    expect(done.result?.backups.length).toBeGreaterThan(0);
  });

  it("test03_a_removal_and_a_forgotten_record_are_plans_of_their_own", () => {
    expect(outcome(removed).action).toBe("removed");
    expect(outcome(forgotten).action).toBe("forgotten");
  });

  it("test04_a_listed_row_carries_the_fields_the_panel_reads", () => {
    const [row] = (list.envelope.data as DirectListData).entries;
    expect(Object.keys(row).sort()).toEqual(
      ["client", "clientName", "entry", "path", "server", "serverName", "state"].sort(),
    );
  });
});
