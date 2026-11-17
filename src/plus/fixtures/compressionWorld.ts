import type { CtlReply } from "./ctlWorld";
import type {
  CompressionDoctorData,
  CompressionStatusData,
  EntryHealth,
  ExportData,
  EntryState,
  HookCounter,
  HookEntry,
  HookProviderId,
  HookWriteRow,
  HooksStatusData,
  MeasureReport,
  ProviderName,
  VerifyData,
  VerifyRow,
} from "../compression/data";

const HOME = "/Users/example";
const SETTINGS = `${HOME}/.claude/settings.json`;
const WRAPPED_AT = "2026-11-05T09:00:00.000Z";
const CANNOT_TELL =
  "Counts hook calls, latency and how often the hook answered. It cannot see the size of the tool output a rewrite later saves; that needs the billed measurement in `compression verify`.";

export type HooksScenario =
  "adopt" | "on" | "paused" | "paused-open" | "missing" | "edited" | "none";
export type MeasureScenario = "refused" | "compared" | "empty";

export interface CompressionWorldOptions {
  /** `adopt`: both hooks are hand-written (the first run). `on`, `paused`, `missing` and
   * `edited` start with the hooks wrapped; `paused` is paused until tomorrow and `paused-open`
   * with no end; `none` has no hook at all. */
  hooks?: HooksScenario;
  /** `fail`: Verify finds that the hooks answer nothing. */
  verify?: "pass" | "fail";
  /** `refused`: too few paused sessions for a verdict. `compared`: a verdict with overlapping
   * middle halves. `empty`: no transcripts. */
  measure?: MeasureScenario;
  /** The policy's provider, for the proxy and hosted tiles. */
  provider?: ProviderName;
}

const ORIGINAL: Record<HookProviderId, string> = {
  rtk: "rtk hook claude",
  "read-once": "~/.claude/hooks/read-once.sh",
};

const wrap = (id: HookProviderId, original: string) =>
  `toolportctl compression hook ${id} -- /bin/sh -c '${original}'`;

interface Slot {
  state: EntryState;
  command: string | null;
  /** What the ledger recorded when the entry was wrapped. */
  recorded: { original: string; wrapped: string; at: string } | null;
  /** Calls were counted before, so the counters stay after an undo. */
  counted: boolean;
  /** What the ledger held before an edit was kept; Undo puts it back. */
  beforeKeep?: { original: string; wrapped: string } | null;
}

const COUNTERS: Record<HookProviderId, Omit<HookCounter, "provider" | "basis">> = {
  rtk: {
    calls: 2100,
    answered: 1842,
    failed: 3,
    medianLatencyMs: 38,
    p95LatencyMs: 140,
    maxLatencyMs: 420,
    firstCall: "2026-11-05",
    lastCall: "2026-11-16",
  },
  "read-once": {
    calls: 340,
    answered: 312,
    failed: 0,
    medianLatencyMs: 41,
    p95LatencyMs: 96,
    maxLatencyMs: 150,
    firstCall: "2026-11-05",
    lastCall: "2026-11-16",
  },
};

function seedSlots(scenario: HooksScenario): Record<HookProviderId, Slot> {
  const foreign = (id: HookProviderId): Slot => ({
    state: "foreign",
    command: ORIGINAL[id],
    recorded: null,
    counted: false,
  });
  const ours = (id: HookProviderId): Slot => ({
    state: "ours",
    command: wrap(id, ORIGINAL[id]),
    recorded: {
      original: ORIGINAL[id],
      wrapped: wrap(id, ORIGINAL[id]),
      at: WRAPPED_AT,
    },
    counted: true,
  });
  const absent: Slot = { state: "absent", command: null, recorded: null, counted: false };
  switch (scenario) {
    case "adopt":
      return { rtk: foreign("rtk"), "read-once": foreign("read-once") };
    case "none":
      return { rtk: { ...absent }, "read-once": { ...absent } };
    case "missing":
      return {
        rtk: { ...ours("rtk"), state: "absent", command: null },
        "read-once": ours("read-once"),
      };
    case "edited":
      return {
        rtk: { ...ours("rtk"), command: `${wrap("rtk", ORIGINAL.rtk)} # tweaked` },
        "read-once": ours("read-once"),
      };
    default:
      return { rtk: ours("rtk"), "read-once": ours("read-once") };
  }
}

function healthOf(slot: Slot): EntryHealth {
  if (slot.state === "foreign") return "foreign";
  if (slot.state === "ours") {
    return slot.recorded && slot.command === slot.recorded.wrapped ? "ok" : "edited";
  }
  return slot.recorded ? "missing" : "absent";
}

const DOCTOR: CompressionDoctorData = {
  provider: "none",
  healthy: true,
  checks: [
    { name: "engine", ok: true, detail: "compression disabled (provider none)" },
    { name: "pin", ok: true, detail: "not used by provider none" },
    { name: "shims", ok: true, detail: "not required for this provider" },
  ],
};

const MEASURED_FROM = "2026-10-16";
const MEASURED_TO = "2026-11-16";

function measureReport(scenario: MeasureScenario): MeasureReport {
  const upstream = {
    note: "Upstream's tokens-saved figure counts tool-definition exposure of MCP servers only. It says nothing about Bash, Read or billed tokens and is never added to the figures here.",
  };
  const transcripts = {
    root: `${HOME}/.claude/projects`,
    sessions: scenario === "empty" ? 0 : 461,
    since: null,
  };
  if (scenario === "empty") {
    return {
      transcripts,
      sources: {
        status: "no_transcripts",
        basis: "estimate",
        method:
          "Size of each tool result divided by 4, times the assistant turns that came after it",
        rows: [],
        bashCeiling: null,
        population: { sessions: 0, from: null, to: null },
        cannotTell: "",
      },
      hooks: {
        status: "off",
        basis: "mechanical",
        providers: [],
        outputBytes: {
          available: false,
          reason:
            "A PreToolUse hook sees the call and its answer, not the tool output a rewrite later shrinks.",
        },
        providerReported: [],
        population: { hooks: 0, calls: 0, from: null, to: null },
        cannotTell: "",
      },
      outcome: {
        status: "no_sessions",
        timeline: [],
        groups: [],
        referenceState: "on",
        verdict: { status: "none", reason: "" },
        population: { sessions: 0, minTurns: 5, unplaced: 0 },
        costUnits: {},
        cannotTell: "",
      },
      rework: {
        status: "no_sessions",
        groups: [],
        population: { sessions: 0, minTurns: 5, unplaced: 0 },
        cannotTell: "",
      },
      upstream,
    };
  }
  const compared = scenario === "compared";
  const population = { sessions: 461, from: MEASURED_FROM, to: MEASURED_TO };
  return {
    transcripts,
    sources: {
      status: "data",
      basis: "estimate",
      method:
        "Size of each tool result divided by 4, times the assistant turns that came after it",
      rows: [
        { kind: "Read", share: 0.38 },
        { kind: "Bash", share: 0.27 },
        { kind: "MCP results", share: 0.19 },
        { kind: "Edit, Write", share: 0.09 },
        { kind: "Other", share: 0.07 },
      ],
      bashCeiling: 0.27,
      population,
      cannotTell:
        "how much of that share a hook removed, or whether the model needed the removed text. Compaction is not modelled, so a long session counts every result for every later turn.",
    },
    hooks: {
      status: "data",
      basis: "mechanical",
      providers: (["rtk", "read-once"] as const).map((provider) => ({
        provider,
        wrapped: true,
        calls: COUNTERS[provider].calls,
        answered: COUNTERS[provider].answered,
        failed: COUNTERS[provider].failed,
        p50Ms: COUNTERS[provider].medianLatencyMs,
        p95Ms: COUNTERS[provider].p95LatencyMs,
        firstCall: COUNTERS[provider].firstCall,
        lastCall: COUNTERS[provider].lastCall,
      })),
      outputBytes: {
        available: false,
        reason:
          "A PreToolUse hook sees the call and its answer, not the tool output a rewrite later shrinks.",
      },
      providerReported: [],
      population: { hooks: 2, calls: 2440, from: "2026-11-05", to: "2026-11-16" },
      cannotTell:
        "billed tokens, or whether an output was cut that Claude then asked for again (see the rework check). A PreToolUse hook sees the call and its answer, not the size of the output a rewrite saves.",
    },
    outcome: {
      status: "data",
      timeline: [
        { state: "off", label: "Hooks off", from: null, to: "2026-11-05" },
        { state: "on", label: "Hooks on", from: "2026-11-05", to: "2026-11-12" },
        { state: "paused", label: "Paused", from: "2026-11-12", to: "2026-11-13" },
        { state: "on", label: "Hooks on", from: "2026-11-13", to: null },
      ],
      groups: [
        {
          state: "off",
          label: "Hooks off",
          intervals: [{ from: null, to: "2026-11-05" }],
          sessions: 96,
          medianCost: 41000,
          lowerQuartile: 30000,
          upperQuartile: 71000,
          relative: 104,
          relativeLow: 76,
          relativeHigh: 180,
        },
        {
          state: "on",
          label: "Hooks on",
          intervals: [
            { from: "2026-11-05", to: "2026-11-12" },
            { from: "2026-11-13", to: null },
          ],
          sessions: 118,
          medianCost: 39400,
          lowerQuartile: 28000,
          upperQuartile: 63000,
          relative: 100,
          relativeLow: 71,
          relativeHigh: 160,
        },
        {
          state: "paused",
          label: "Paused",
          intervals: [{ from: "2026-11-12", to: "2026-11-13" }],
          sessions: compared ? 12 : 4,
          medianCost: 44100,
          lowerQuartile: 31000,
          upperQuartile: 66000,
          relative: 112,
          relativeLow: 79,
          relativeHigh: 168,
        },
      ],
      referenceState: "on",
      verdict: compared
        ? {
            status: "compared",
            overlap: true,
            reason:
              "The middle halves of both windows overlap: no difference can be told from these sessions.",
          }
        : {
            status: "refused",
            needsSessions: 8,
            reason:
              "Needs at least 8 sessions with 5 or more turns in each window; hooks on has 118, paused has 4.",
          },
      population: { sessions: 218, minTurns: 5, unplaced: 0 },
      costUnits: {
        inputToken: 1,
        cacheWriteToken: 1.25,
        cacheReadToken: 0.1,
        outputToken: 5,
        relativeTo: "the median of the hooks-on window = 100",
      },
      cannotTell:
        "that a paused window is a fair comparison: the task mix of two windows is not the same, and a refusal is shown instead of a percentage when the windows are thin.",
    },
    rework: {
      status: "data",
      groups: [
        {
          state: "off",
          label: "Hooks off",
          sessions: 96,
          repeatedCommands: 1.8,
          repeatedReads: 3.1,
        },
        {
          state: "on",
          label: "Hooks on",
          sessions: 118,
          repeatedCommands: 1.9,
          repeatedReads: 0.8,
        },
        {
          state: "paused",
          label: "Paused",
          sessions: compared ? 12 : 4,
          repeatedCommands: 1.7,
          repeatedReads: 3.4,
        },
      ],
      population: { sessions: 218, minTurns: 5, unplaced: 0 },
      cannotTell:
        "whether any single repeat was caused by compression. Treat both figures as hints.",
    },
    upstream,
  };
}

const REWRITE = '{"hookSpecificOutput":{"updatedInput":{"command":"rtk git status"}}}';
const BLOCK = '{"decision":"block"}';

const ok = (data: unknown): CtlReply => ({ ok: true, data });
const fail = (code: string, message: string): CtlReply => ({
  ok: false,
  error: { code, message },
});

/** An in-memory `toolportctl compression`: the hook entries of one settings file with the ledger
 * beside them, the pause, and the status, doctor and measure answers. The rows of a write follow
 * the real command's actions. */
export function createCompressionWorld(options: CompressionWorldOptions = {}) {
  const scenario = options.hooks ?? "adopt";
  const slots = seedSlots(scenario);
  const HOUR = 3_600_000;
  let paused = scenario === "paused" || scenario === "paused-open";
  let pausedSince: string | null = paused
    ? new Date(Date.now() - 4 * HOUR).toISOString()
    : null;
  let pausedUntil: string | null =
    scenario === "paused" ? new Date(Date.now() + 20 * HOUR).toISOString() : null;
  let epochs = scenario === "adopt" || scenario === "none" ? 0 : 3;

  const ids = Object.keys(slots) as HookProviderId[];

  function entries(): HookEntry[] {
    return ids.map((provider) => ({
      file: SETTINGS,
      provider,
      state: slots[provider].state,
      command: slots[provider].command,
      health: healthOf(slots[provider]),
      wrappedAt: slots[provider].recorded?.at ?? null,
    }));
  }

  function status(): HooksStatusData {
    return {
      paused,
      pausedSince,
      pausedUntil,
      entries: entries(),
      counters: ids.map((provider) => ({
        provider,
        basis: "mechanical",
        ...(slots[provider].counted
          ? COUNTERS[provider]
          : {
              calls: 0,
              answered: 0,
              failed: 0,
              medianLatencyMs: null,
              p95LatencyMs: null,
              maxLatencyMs: null,
              firstCall: null,
              lastCall: null,
            }),
      })),
      wrapped: ids.filter((id) => slots[id].recorded).length,
      epochs,
      cannotTell: CANNOT_TELL,
      trust: (
        [
          ["headroom", false, false, false],
          ["rtk-only", false, true, false],
          ["parsec", true, true, true],
          ["none", false, false, false],
        ] as const
      ).map(([provider, closedEngine, installsGlobalHooks, egress]) => ({
        provider,
        trust: {
          closedEngine,
          installsGlobalHooks,
          needsEgressConsent: egress,
          sendsCodeOffMachine: egress,
        },
      })),
    };
  }

  function apply(only: string | null, dryRun: boolean): CtlReply {
    const rows: HookWriteRow[] = [];
    for (const provider of ids) {
      if (only && only !== provider) continue;
      const slot = slots[provider];
      const row = (action: HookWriteRow["action"], state: EntryState): HookWriteRow => ({
        file: SETTINGS,
        provider,
        action,
        state,
        command: slot.command,
      });
      if (slot.state === "absent" && !slot.recorded) {
        rows.push(row("none to wrap", "absent"));
      } else if (slot.state === "absent" && slot.recorded) {
        rows.push(row(dryRun ? "would re-apply" : "re-applied", "absent"));
        if (!dryRun) {
          slot.state = "ours";
          slot.command = slot.recorded.wrapped;
        }
      } else if (slot.state === "ours" && slot.recorded) {
        if (slot.command === slot.recorded.wrapped) {
          rows.push(row("kept", "ours"));
        } else {
          rows.push(row(dryRun ? "would restore ours" : "restored ours", "ours"));
          if (!dryRun) slot.command = slot.recorded.wrapped;
        }
      } else if (slot.state === "foreign") {
        rows.push(row(dryRun ? "would wrap" : "wrapped", "foreign"));
        if (!dryRun) {
          const wrapped = wrap(provider, slot.command ?? ORIGINAL[provider]);
          slot.recorded = {
            original: slot.command ?? ORIGINAL[provider],
            wrapped,
            at: "2026-11-17T09:12:00.000Z",
          };
          slot.state = "ours";
          slot.command = wrapped;
          epochs += 1;
        }
      }
    }
    return ok({ dryRun, rows });
  }

  function undo(only: string | null, dryRun: boolean): CtlReply {
    const rows: HookWriteRow[] = [];
    for (const provider of ids) {
      const slot = slots[provider];
      if ((only && only !== provider) || !slot.recorded) continue;
      const base = { file: SETTINGS, provider };
      if (slot.state === "ours" && slot.command === slot.recorded.wrapped) {
        rows.push({ ...base, action: dryRun ? "would restore" : "restored" });
        if (!dryRun) {
          slot.state = "foreign";
          slot.command = slot.recorded.original;
          slot.recorded = null;
        }
      } else if (slot.state === "ours") {
        rows.push({ ...base, action: "edited since, left alone" });
      } else {
        rows.push({ ...base, action: "gone, dropped from the ledger" });
        if (!dryRun) slot.recorded = null;
      }
    }
    if (!dryRun && rows.length > 0) epochs += 1;
    return ok({ dryRun, rows });
  }

  function pause(on: boolean, duration: string | null): CtlReply {
    const changed = paused !== on || (on && duration !== null);
    paused = on;
    pausedSince = on ? new Date().toISOString() : null;
    pausedUntil =
      on && duration === "1d" ? new Date(Date.now() + 24 * HOUR).toISOString() : null;
    if (paused !== on || changed) epochs += 1;
    return ok({ paused: on, changed, pausedSince, pausedUntil });
  }

  function adoptEdit(id: HookProviderId, revert: boolean): CtlReply {
    const slot = slots[id];
    const rows = (action: "adopted" | "reverted") => ({
      provider: id,
      revert,
      rows: [{ file: SETTINGS, provider: id, action, command: slot.command }],
    });
    if (revert) {
      if (!slot.beforeKeep || !slot.recorded) {
        return fail("hooks_failed", `no ${id} entry holds an adopted edit to revert`);
      }
      slot.recorded = { ...slot.recorded, ...slot.beforeKeep };
      slot.beforeKeep = null;
      return ok(rows("reverted"));
    }
    if (healthOf(slot) !== "edited" || !slot.recorded || !slot.command) {
      return fail("hooks_failed", `no ${id} entry reads as edited`);
    }
    slot.beforeKeep = {
      original: slot.recorded.original,
      wrapped: slot.recorded.wrapped,
    };
    slot.recorded = { ...slot.recorded, original: slot.command, wrapped: slot.command };
    return ok(rows("adopted"));
  }

  function verify(only: string | null): CtlReply {
    const failing = options.verify === "fail";
    const rows: VerifyRow[] = ids
      .filter((id) => !only || only === id)
      .map((provider) => {
        const slot = slots[provider];
        const base = { file: SETTINGS, provider, command: slot.command ?? undefined };
        if (slot.state === "absent") {
          return {
            ...base,
            status: "skipped",
            reason: "no entry in this settings file",
            calls: [],
          };
        }
        const step =
          provider === "rtk"
            ? [
                {
                  step: "Bash: git status",
                  answered: !failing,
                  stdout: failing ? "" : REWRITE,
                },
              ]
            : [
                { step: "Read: first read", answered: false, stdout: "" },
                {
                  step: "Read: repeat read",
                  answered: !failing,
                  stdout: failing ? "" : BLOCK,
                },
              ];
        const calls = step.map((c, i) => ({
          ...c,
          exitCode: 0,
          latencyMs: provider === "rtk" ? 38 : 41 + i,
          stderr: "",
        }));
        return {
          ...base,
          wrapped: slot.state === "ours",
          paused,
          status: failing ? "fail" : "pass",
          reason: failing
            ? provider === "rtk"
              ? "the hook answered nothing; rtk may be missing or may not rewrite this command"
              : "the hook let a repeat read of an unchanged file through without an answer"
            : null,
          returned: failing
            ? null
            : provider === "rtk"
              ? "rewrote `git status` to `rtk git status`"
              : `answered the repeat read: ${BLOCK}`,
          latencyMs: Math.max(...calls.map((c) => c.latencyMs)),
          calls,
        };
      });
    const count = (status: string) => rows.filter((r) => r.status === status).length;
    const data: VerifyData = {
      payload:
        "A synthetic PreToolUse event on stdin, a temporary working directory, no network.",
      passed: count("pass"),
      failed: count("fail"),
      skipped: count("skipped"),
      paused,
      rows,
    };
    return failing
      ? {
          ok: false,
          data,
          error: { code: "unhealthy", message: "one or more checks failed" },
        }
      : ok(data);
  }

  function compressionStatus(): CompressionStatusData {
    return {
      configExists: true,
      provider: options.provider ?? "none",
      runtime: "none",
      preset: { name: "interactive", mode: "cache", port: 8787 },
      pin: {
        pin: "0.29.0",
        installed: null,
        drift: null,
        requirement: "headroom-ai[proxy,code,ml]==0.29.0",
      },
      shims: {
        exists: false,
        path: `${HOME}/Library/Application Support/Toolport/compression-shims.zsh`,
      },
    };
  }

  return {
    run(argv: string[]): CtlReply {
      const [, sub, verb] = argv;
      const flags = argv.slice(sub === "hooks" ? 3 : 2);
      const provider = flags[flags.indexOf("--provider") + 1] ?? null;
      const only = flags.includes("--provider") ? provider : null;
      const dry = flags.includes("--dry-run");
      if (sub === "hooks") {
        if (verb === undefined || verb === "status") return ok(status());
        if (verb === "apply") return apply(only, dry);
        if (verb === "undo") return undo(only, dry);
        if (verb === "pause")
          return pause(
            true,
            flags.includes("--for") ? flags[flags.indexOf("--for") + 1] : null,
          );
        if (verb === "resume") return pause(false, null);
        if (verb === "adopt-edit")
          return adoptEdit(flags[0] as HookProviderId, flags.includes("--revert"));
        if (verb === "verify") return verify(flags[0] ?? null);
      }
      if (sub === "status") return ok(compressionStatus());
      if (sub === "doctor") return ok(DOCTOR);
      if (sub === "measure") {
        const out = flags[flags.indexOf("--out") + 1];
        if (flags.includes("--out")) {
          return ok({ format: "csv", rows: 31, out, bytes: 5120 } satisfies ExportData);
        }
        return ok(measureReport(options.measure ?? "refused"));
      }
      return fail(
        "usage",
        `Unimplemented fixture command: compression ${argv.slice(1).join(" ")}`,
      );
    },
  };
}
