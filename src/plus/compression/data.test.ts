import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createCompressionWorld } from "../fixtures/compressionWorld";
import type {
  CompressionDoctorData,
  CompressionStatusData,
  HooksStatusData,
  HooksWriteData,
  MeasureReport,
} from "./data";

const root = join(__dirname, "../../../plus/fixtures");

interface Golden {
  argv: string[];
  envelope: { ok: boolean; data?: unknown };
}

function ctl(name: string): Golden {
  return JSON.parse(
    readFileSync(join(root, "ctl-envelopes", `${name}.json`), "utf8"),
  ) as Golden;
}

const keys = (value: unknown) => Object.keys(value as object).sort();

function shape(value: unknown): unknown {
  if (Array.isArray(value)) return value.length > 0 ? [shape(value[0])] : [];
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, inner]) => [key, shape(inner)]),
    );
  }
  return value === null ? "null" : typeof value;
}

describe("the compression envelope types match the Rust goldens", () => {
  it("test01_hooks_status_has_the_fields_the_cards_read", () => {
    const data = ctl("compression-hooks-status").envelope.data as HooksStatusData;
    expect(keys(data)).toEqual([
      "cannotTell",
      "counters",
      "entries",
      "epochs",
      "paused",
      "pausedSince",
      "trust",
      "wrapped",
    ]);
    expect(keys(data.entries[0])).toEqual([
      "command",
      "file",
      "health",
      "provider",
      "state",
      "wrappedAt",
    ]);
    expect(keys(data.counters[0])).toEqual([
      "answered",
      "basis",
      "calls",
      "failed",
      "firstCall",
      "lastCall",
      "maxLatencyMs",
      "medianLatencyMs",
      "p95LatencyMs",
      "provider",
    ]);
  });

  it("test02_a_wrapped_entry_is_ok_and_a_lost_one_is_missing", () => {
    const after = ctl("compression-hooks-apply.after").envelope.data as HooksStatusData;
    expect(after.entries[0]).toMatchObject({ health: "ok", state: "ours" });
    const lost = ctl("compression-hooks-apply.lost").envelope.data as HooksStatusData;
    expect(lost.entries[0]).toMatchObject({ health: "missing", state: "absent" });
    const reapply = ctl("compression-hooks-apply.reapply").envelope
      .data as HooksWriteData;
    expect(reapply.rows[0]).toMatchObject({ action: "re-applied" });
  });

  it("test03_the_policy_and_the_doctor_have_the_fields_the_tab_reads", () => {
    const status = ctl("compression-status").envelope.data as CompressionStatusData;
    expect(keys(status.preset)).toEqual(expect.arrayContaining(["mode", "name", "port"]));
    expect(keys(status.pin)).toEqual(
      expect.arrayContaining(["drift", "installed", "pin", "requirement"]),
    );
    const doctor = ctl("compression-doctor").envelope.data as CompressionDoctorData;
    expect(keys(doctor)).toEqual(
      expect.arrayContaining(["checks", "healthy", "provider"]),
    );
  });

  it("test04_the_measure_report_has_the_four_sections_and_the_fixture_world_has_the_same_shape", () => {
    const golden = ctl("compression-measure.measured").envelope.data as MeasureReport;
    expect(keys(golden)).toEqual([
      "hooks",
      "outcome",
      "rework",
      "sources",
      "transcripts",
      "upstream",
    ]);
    const world = createCompressionWorld({ hooks: "on" }).run(["compression", "measure"]);
    const fixture = world.data as MeasureReport;
    expect(shape(fixture.sources)).toEqual(shape(golden.sources));
    expect(shape(fixture.hooks)).toEqual(shape(golden.hooks));
    expect(shape(fixture.outcome.groups)).toEqual(shape(golden.outcome.groups));
    expect(shape(fixture.outcome.verdict)).toEqual(
      shape({ ...golden.outcome.verdict, needsSessions: 8 }),
    );
    expect(shape(fixture.rework.groups)).toEqual(shape(golden.rework.groups));
  });

  it("test05_the_self_mcp_tool_returns_the_same_report_as_the_command", () => {
    const dir = join(root, "selfmcp-envelopes");
    expect(readdirSync(dir)).toContain("compression_measure.measured.json");
    const tool = JSON.parse(
      readFileSync(join(dir, "compression_measure.measured.json"), "utf8"),
    ) as { result: unknown };
    expect(tool.result).toEqual(ctl("compression-measure.measured").envelope.data);
  });

  it("test06_the_fixture_hooks_status_has_the_same_shape_as_the_golden", () => {
    const golden = ctl("compression-hooks-status").envelope.data;
    const world = createCompressionWorld().run(["compression", "hooks", "status"]);
    expect(shape((world.data as HooksStatusData).entries)).toEqual(
      shape((golden as HooksStatusData).entries),
    );
    expect(shape((world.data as HooksStatusData).trust)).toEqual(
      shape((golden as HooksStatusData).trust),
    );
    expect(keys((world.data as HooksStatusData).counters[0])).toEqual(
      keys((golden as HooksStatusData).counters[0]),
    );
  });
});
