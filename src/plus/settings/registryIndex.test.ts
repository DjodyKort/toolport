import { describe, expect, it } from "vitest";
import indexed from "../../../plus/fixtures/ctl-envelopes/catalog-status.indexed.json";
import stopped from "../../../plus/fixtures/ctl-envelopes/catalog-status.stopped.json";
import { indexNote, indexSummary, type RegistryIndexData } from "./registryIndex";

const FIELDS: (keyof RegistryIndexData)[] = [
  "background",
  "count",
  "error",
  "lastAttemptAt",
  "lastSyncedAt",
  "resumable",
  "state",
];

describe("registry index status", () => {
  it("test01_the_type_names_every_field_of_the_rust_golden", () => {
    for (const golden of [indexed, stopped]) {
      const data = golden.envelope.data as Record<string, unknown>;
      expect(Object.keys(data).filter((key) => key !== "error" || data.error)).toEqual(
        FIELDS.filter((key) => key !== "error" || data.error),
      );
    }
  });

  it("test02_the_summary_counts_servers_and_says_how_old_the_sync_is", () => {
    const now = Date.parse("2026-11-18T12:00:00Z");
    const data = { ...(indexed.envelope.data as RegistryIndexData) };
    data.lastSyncedAt = "2026-11-18T09:00:00Z";
    expect(indexSummary(data, now)).toBe("2 servers, synced 3 h ago");
    expect(indexSummary({ ...data, count: 0, lastSyncedAt: null }, now)).toBe(
      "Not synced yet",
    );
    expect(indexSummary({ ...data, count: 1, lastSyncedAt: null }, now)).toBe(
      "1 server, never fully synced",
    );
  });

  it("test03_the_note_explains_a_stale_index_and_surfaces_the_error", () => {
    const data = indexed.envelope.data as RegistryIndexData;
    expect(indexNote({ ...data, state: "fresh" })).toBeNull();
    expect(indexNote({ ...data, state: "stale" })).toMatch(/live registry/);
    expect(indexNote(stopped.envelope.data as RegistryIndexData)).toMatch(
      /did not answer/,
    );
    expect(indexNote({ ...data, state: "fresh", background: false })).toMatch(
      /Background sync is off/,
    );
  });
});
