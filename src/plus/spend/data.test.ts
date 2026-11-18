import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type {
  ConfigSetData,
  PruneData,
  SetupApplyData,
  SetupPlanData,
  SetupUndoData,
  StatusData,
  SummaryData,
} from "./data";

const dir = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

interface Golden {
  argv: string[];
  envelope: { ok: boolean; data?: unknown };
}

const goldens = (prefix: string) =>
  readdirSync(dir)
    .filter((file) => file.startsWith(prefix) && file.endsWith(".json"))
    .map((file) => ({
      file,
      ...(JSON.parse(readFileSync(join(dir, file), "utf8")) as Golden),
    }))
    .filter((g) => g.envelope.ok && g.envelope.data);

const keys = (value: unknown) => Object.keys(value as object).sort();

const STATUS_KEYS = [
  "enabled",
  "lastEvent",
  "profilesSetUp",
  "receiver",
  "setup",
  "showCost",
  "state",
  "store",
  "transcripts",
];

const SUMMARY_KEYS = [
  "byDay",
  "byModel",
  "byProject",
  "byTask",
  "cacheShare",
  "mcpPart",
  "notes",
  "previous",
  "project",
  "range",
  "since",
  "sources",
  "stale",
  "status",
  "tiles",
  "totals",
];

describe("the spend envelope types match the Rust goldens", () => {
  it("test01_status_carries_the_fields_the_page_reads", () => {
    const all = goldens("spend-status.");
    expect(all.length).toBeGreaterThan(2);
    for (const { envelope, file } of all) {
      const data = envelope.data as StatusData;
      expect(keys(data), file).toEqual(STATUS_KEYS);
      expect(["off", "waiting", "collecting", "not-listening", "error"]).toContain(
        data.state,
      );
      expect(keys(data.transcripts)).toEqual(["done", "files"]);
    }
  });

  it("test02_summary_carries_the_report_and_the_status_it_was_read_with", () => {
    const all = goldens("spend-summary.").filter((g) => g.argv[1] === "summary");
    expect(all.length).toBeGreaterThan(3);
    for (const { envelope, file } of all) {
      const data = envelope.data as SummaryData;
      expect(
        keys(data).filter((k) => k !== "sessionList"),
        file,
      ).toEqual(SUMMARY_KEYS);
      expect(keys(data.tiles)).toEqual(["30d", "7d", "today"]);
      expect(keys(data.status)).toEqual(STATUS_KEYS);
      expect(keys(data.totals)).toEqual([
        "cost",
        "key",
        "requests",
        "sessions",
        "share",
        "tokens",
      ]);
      for (const day of data.byDay) expect(typeof day.byModel).toBe("object");
    }
  });

  it("test03_a_project_report_adds_the_session_list_and_nothing_else", () => {
    const [project] = goldens("spend-summary.month-project");
    const data = project.envelope.data as SummaryData;
    expect(data.project).toBe("/work/alpha");
    expect(data.sessionList?.[0]).toMatchObject({ source: "otel" });
    const [plain] = goldens("spend-summary.default");
    expect((plain.envelope.data as SummaryData).sessionList).toBeUndefined();
  });

  it("test04_the_task_split_names_interactive_sessions_apart_from_unattributed", () => {
    const [plain] = goldens("spend-summary.default");
    const tasks = (plain.envelope.data as SummaryData).byTask.map((t) => t.key);
    expect(tasks).toContain("Interactive");
    expect(tasks).not.toContain("Unattributed");
  });

  it("test05_the_mcp_part_has_the_ready_shape_the_card_reads", () => {
    const [plain] = goldens("spend-summary.default");
    const part = (plain.envelope.data as SummaryData).mcpPart;
    expect(part.state).toBe("ready");
    if (part.state === "error") return;
    expect(keys(part)).toEqual(
      [
        "compression",
        "firstRequest",
        "note",
        "savedTokens",
        "shareOfInputTokens",
        "state",
        "toolDefinitions",
      ].sort(),
    );
    expect(keys(part.toolDefinitions)).toEqual(
      [
        "badge",
        "exposedTokens",
        "fullTokens",
        "medianExposedTokens",
        "medianFullTokens",
        "sessions",
        "tokenizer",
      ].sort(),
    );
  });

  it("test06_a_plan_lists_every_key_per_profile_with_what_will_happen", () => {
    const all = goldens("spend-setup-plan.");
    expect(all.length).toBeGreaterThan(2);
    for (const { envelope, file } of all) {
      const plan = envelope.data as SetupPlanData;
      expect(plan.contentFlags, file).toBe("never written");
      for (const profile of plan.profiles) {
        expect(profile.entries).toHaveLength(5);
        for (const entry of profile.entries)
          expect(["add", "update", "already set", "conflict", "overwrite"]).toContain(
            entry.action,
          );
      }
    }
  });

  it("test07_apply_reports_completeness_and_undo_reports_what_was_kept", () => {
    const applied = goldens("spend-setup-apply.conflict")[0].envelope
      .data as SetupApplyData;
    expect(applied).toMatchObject({ complete: false, enabled: true });
    const undone = goldens("spend-setup-undo.undo-delete-data")[0].envelope
      .data as SetupUndoData;
    expect(undone.deleted).toMatchObject({ events: expect.any(Number) });
    expect(keys(undone.profiles[0])).toEqual(["keptChanged", "profile", "restored"]);
  });

  it("test08_prune_and_config_answer_with_the_numbers_the_dialogs_show", () => {
    const prune = goldens("spend-prune.dry-run")[0].envelope.data as PruneData;
    expect(keys(prune)).toEqual(
      [
        "dryRun",
        "foldedDays",
        "keptBytes",
        "keptEvents",
        "maxBytes",
        "removedBytes",
        "removedEvents",
        "retentionDays",
        "totalsDeleted",
      ].sort(),
    );
    const config = goldens("spend-config-set.save")[0].envelope.data as ConfigSetData;
    expect(keys(config)).toEqual(
      [
        "changed",
        "enabled",
        "maxMb",
        "port",
        "portChanged",
        "retentionDays",
        "showCost",
      ].sort(),
    );
  });
});
