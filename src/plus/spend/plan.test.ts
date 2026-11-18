import { describe, expect, it } from "vitest";
import { createSpendWorld, PROFILE_PERSONAL, PROFILE_WORK } from "../fixtures/spendWorld";
import type { SetupPlanData } from "./data";
import { boundedInt } from "./model";
import {
  chosenProfiles,
  conflictKeys,
  conflictValue,
  diffLines,
  selectedProfiles,
  validPort,
  writeCount,
} from "./plan";

function plan(conflict = false): SetupPlanData {
  const reply = createSpendWorld({ scenario: "never", conflict }).run([
    "spend",
    "setup",
    "plan",
  ]);
  if (!reply.ok) throw new Error(reply.error?.message);
  return reply.data as SetupPlanData;
}

describe("the setup plan helpers", () => {
  it("test01_every_add_is_a_write_and_a_conflict_is_one_only_when_replaced", () => {
    const { profiles } = plan(true);
    expect(writeCount(profiles, [])).toBe(9);
    expect(writeCount(profiles, ["OTEL_METRICS_EXPORTER"])).toBe(10);
    expect(conflictKeys(profiles)).toEqual(["OTEL_METRICS_EXPORTER"]);
    expect(conflictValue(profiles, "OTEL_METRICS_EXPORTER")).toBe("prometheus");
  });

  it("test02_leaving_every_profile_ticked_passes_no_profile_flag", () => {
    const { profiles } = plan();
    expect(chosenProfiles(profiles, [])).toEqual([]);
    expect(chosenProfiles(profiles, [PROFILE_PERSONAL])).toEqual([PROFILE_WORK]);
    expect(selectedProfiles(profiles, [PROFILE_WORK]).map((p) => p.profile)).toEqual([
      PROFILE_PERSONAL,
    ]);
  });

  it("test03_a_skipped_conflict_shows_the_kept_value_and_a_replaced_one_the_swap", () => {
    const entry = plan(true).profiles[0].entries.find((e) => e.action === "conflict")!;
    expect(diffLines(entry, false).map((l) => l.sign)).toEqual([" ", " "]);
    expect(diffLines(entry, true).map((l) => l.sign)).toEqual(["-", "+"]);
    expect(diffLines(entry, true)[0].text).toContain("prometheus");
  });

  it("test04_ports_and_bounded_numbers_reject_what_the_command_would", () => {
    expect(validPort("4318")).toBe(4318);
    expect(["80", "1023", "65536", "43.5", "abc", ""].map(validPort)).toEqual(
      Array(6).fill(null),
    );
    expect(boundedInt("30", 7, 90)).toBe(30);
    expect(["6", "91", "", " ", "1e1"].map((v) => boundedInt(v, 7, 90))).toEqual([
      null,
      null,
      null,
      null,
      null,
    ]);
  });
});
