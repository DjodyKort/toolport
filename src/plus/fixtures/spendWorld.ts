import type {
  Badge,
  ConfigSetData,
  DayRow,
  McpPart,
  Money,
  PlanEntry,
  PlanProfile,
  PruneData,
  RangeId,
  SessionRow,
  SpendRow,
  StatusData,
  SummaryData,
  TokenCounts,
} from "../spend/data";
import type { CtlReply } from "./ctlWorld";

/** Wednesday 18 November 2026, 12:30 UTC, the day of the design's screenshots. */
export const SPEND_NOW = Date.parse("2026-11-18T12:30:00Z");

export const FIXTURE_HOME = "/Users/example";
export const PROFILE_PERSONAL = `${FIXTURE_HOME}/.claude/settings.json`;
export const PROFILE_WORK = `${FIXTURE_HOME}/.claude-work/settings.json`;
export const PROJECT_WEB = `${FIXTURE_HOME}/work/clients/alpha/web-app`;

const DAY = 86_400_000;
const MB = 1024 * 1024;
const TOKENS_PER_USD = 3_460_000;
const TRANSCRIPT_FILES = 461;
const SCAN_STEP = 77;

export type SpendScenario =
  "healthy" | "never" | "off" | "waiting" | "computing" | "cannot-listen";

export interface SpendWorldOptions {
  /** Where collecting stands when the page opens. */
  scenario?: SpendScenario;
  /** Adds the not-exporting data: sessions ran after the last event. */
  stale?: boolean;
  /** The MCP part cannot read upstream's savings log. */
  mcpError?: boolean;
  /** `spend summary` fails with this message. */
  summaryError?: string;
  /** Milliseconds the first `spend summary` takes while transcripts are read; default 4000. */
  scanMs?: number;
  /** The personal profile already sets another exporter, so a plan has a conflict. */
  conflict?: boolean;
  /** Dollar figures are switched off in Settings. */
  hideCost?: boolean;
}

type Phase = "never" | "off" | "waiting" | "healthy" | "cannot-listen";

const PHASE_OF: Record<SpendScenario, Phase> = {
  healthy: "healthy",
  never: "never",
  off: "off",
  waiting: "waiting",
  computing: "off",
  "cannot-listen": "cannot-listen",
};

const iso = (at: number) => new Date(at).toISOString().replace(/\.\d+Z$/, "Z");
const dayOf = (at: number) => new Date(at).toISOString().slice(0, 10);

const fail = (code: string, message: string): CtlReply => ({
  ok: false,
  error: { code, message },
});

const flag = (argv: string[], name: string) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv[at + 1] : undefined;
};

const flags = (argv: string[], name: string) =>
  argv.flatMap((arg, at) => (arg === name ? [argv[at + 1]] : []));

const MODEL_SPLIT: [string, number][] = [
  ["claude-opus-5", 0.64],
  ["claude-sonnet-5-5", 0.34],
  ["claude-haiku-5", 0.02],
];

/** USD per day, oldest first, for the last seven days; the earlier days repeat a quiet pattern. */
const LAST_WEEK = [19.8, 27.1, 6.9, 4.2, 38.6, 56.2, 31.4];
const EARLIER = [12.4, 18.9, 22.3, 9.1, 3.8, 14.7, 25.5];
/** The two days the app was closed: transcripts only, so estimated all through. */
const ESTIMATED_DAYS = new Set(["2026-11-14", "2026-11-15"]);

function counts(usd: number, estimated: number): TokenCounts {
  const total = Math.round(usd * TOKENS_PER_USD);
  return {
    input: Math.round(total * 0.005),
    output: Math.round(total * 0.003),
    cacheRead: Math.round(total * 0.958),
    cacheCreation: Math.round(total * 0.034),
    total,
    badge: "Measured",
    fromTranscript: Math.round(total * (usd > 0 ? estimated / usd : 0)),
  };
}

function cost(usd: number, estimatedUsd: number): Money {
  return {
    usd: Math.round(usd * 1e6) / 1e6,
    estimatedUsd: Math.round(estimatedUsd * 1e6) / 1e6,
    badge: estimatedUsd > 0 || usd === 0 ? "Estimate" : "Measured",
  };
}

function spendRow(
  key: string,
  usd: number,
  estimated: number,
  grand: number,
  sessions: number,
  requests: number,
): SpendRow {
  return {
    key,
    tokens: counts(usd, estimated),
    cost: cost(usd, estimated),
    requests,
    sessions,
    share: grand > 0 ? Math.round((usd / grand) * 1000) / 1000 : 0,
  };
}

interface Day {
  key: string;
  usd: number;
  estimated: number;
}

function days(count: number, transcriptsOnly: boolean): Day[] {
  const today = Math.floor(SPEND_NOW / DAY) * DAY;
  return Array.from({ length: count }, (_, index) => {
    const back = count - 1 - index;
    const key = dayOf(today - back * DAY);
    const usd = back < 7 ? LAST_WEEK[6 - back] : EARLIER[back % EARLIER.length];
    const estimated = transcriptsOnly || ESTIMATED_DAYS.has(key) ? usd : 0;
    return { key, usd, estimated };
  });
}

const PROJECTS: [string, number, number][] = [
  [PROJECT_WEB, 0.39, 41],
  [`${FIXTURE_HOME}/work/clients/alpha/api`, 0.22, 27],
  [`${FIXTURE_HOME}/work/internal/platform`, 0.15, 22],
  [`${FIXTURE_HOME}/study/internship`, 0.11, 18],
  [`${FIXTURE_HOME}/work/clients/beta/portal`, 0.08, 14],
  [`${FIXTURE_HOME}/old/one`, 0.02, 6],
  [`${FIXTURE_HOME}/old/two`, 0.02, 5],
  [`${FIXTURE_HOME}/old/three`, 0.01, 9],
];

const TASKS: [string, number, number][] = [
  ["Interactive", 0.89, 118],
  ["nightly-review", 0.06, 7],
  ["docs-sync", 0.03, 14],
  ["weekly-context-measure", 0.01, 1],
  ["Unattributed", 0.01, 2],
];

function tilesOf(
  phase: Phase,
  hideCost: boolean,
): {
  tokens: number;
  usd: number;
  est: number;
  previous: number;
  delta: number | null;
  estDays: number;
}[] {
  void hideCost;
  const transcriptsOnly = phase !== "healthy" && phase !== "cannot-listen";
  const build = (count: number) => {
    const list = days(count, transcriptsOnly);
    const before = Array.from({ length: count }, (_, index) =>
      count === 1
        ? LAST_WEEK[5]
        : EARLIER[index % EARLIER.length] * (count === 7 ? 3.4 : 1.4),
    );
    const usd = list.reduce((sum, d) => sum + d.usd, 0);
    const est = list.reduce((sum, d) => sum + d.estimated, 0);
    const previous = before.reduce((sum, v) => sum + v, 0);
    return {
      tokens: Math.round(usd * TOKENS_PER_USD),
      usd,
      est,
      previous,
      delta: count === 1 ? null : Math.round((usd / previous - 1) * 1000) / 1000,
      estDays: list.filter((d) => d.estimated > 0).length,
    };
  };
  return [build(1), build(7), build(30)];
}

const RANGE_DAYS: Record<RangeId, number> = { today: 1, "7d": 7, "30d": 30 };

export function createSpendWorld(options: SpendWorldOptions = {}) {
  const scenario = options.scenario ?? "healthy";
  let phase: Phase = PHASE_OF[scenario];
  let port = 4318;
  let retentionDays = 30;
  let maxMb = 64;
  let showCost = options.hideCost !== true;
  let scanned = scenario !== "computing";
  let statusCalls = 0;
  let scanTimer: ReturnType<typeof setTimeout> | null = null;
  let dataDeleted = false;
  const setUp = new Map<string, string>(
    phase === "never" || phase === "off"
      ? []
      : [[PROFILE_PERSONAL, "2026-11-02T09:00:00Z"]],
  );
  const files = phase === "never" ? 0 : TRANSCRIPT_FILES;

  const hasData = () =>
    !dataDeleted &&
    (phase === "healthy" || phase === "cannot-listen" || phase === "waiting");

  function status(): StatusData {
    statusCalls += 1;
    const enabled = phase !== "never" && phase !== "off";
    const stateOf = {
      never: "off",
      off: "off",
      waiting: "waiting",
      healthy: "collecting",
      "cannot-listen": "error",
    } as const;
    const stored = hasData() && phase !== "waiting";
    return {
      state: stateOf[phase],
      enabled,
      receiver: {
        address: `127.0.0.1:${port}`,
        listening: phase === "healthy" || phase === "waiting",
        owner: phase === "healthy" || phase === "waiting" ? "app" : null,
        requests: stored ? 8012 : 0,
        events: stored ? 41206 : 0,
        rejected: 0,
        dropped: 0,
        error:
          phase === "cannot-listen" ? `Port ${port} is used by another program.` : null,
      },
      lastEvent: stored ? iso(SPEND_NOW - 14_000) : null,
      store: {
        events: stored ? 41206 : 0,
        bytes: stored ? Math.round(18.4 * MB) : 0,
        maxBytes: maxMb * MB,
        oldest: stored ? iso(SPEND_NOW - 16 * DAY) : null,
        retentionDays,
        dailyLines: stored ? 62 : 0,
        skippedLines: 0,
      },
      showCost,
      profilesSetUp: setUp.size,
      setup: [...setUp].map(([profile, appliedAt]) => ({ profile, appliedAt })),
      transcripts: {
        files,
        done: scanned ? files : Math.min(files - 1, statusCalls * SCAN_STEP),
      },
    };
  }

  function mcpPart(): McpPart {
    if (options.mcpError)
      return {
        state: "error",
        error: "savings-v4.jsonl: line 812 is not valid JSON.",
      };
    const badge: Badge = "Counted";
    return {
      state: "ready",
      toolDefinitions: {
        sessions: 142,
        exposedTokens: 142 * 4100,
        fullTokens: 142 * 61_300,
        medianExposedTokens: 4100,
        medianFullTokens: 61_300,
        tokenizer: "cl100k_base",
        badge,
      },
      shareOfInputTokens: { value: 0.0635, badge: "Estimate" },
      savedTokens: 11_400_000,
      compression: {
        hooks: 2,
        calls: 2154,
        firstCall: "2026-11-05T08:00:00Z",
        lastCall: "2026-11-18T12:00:00Z",
        tokensCut: 1_310_000,
        badge,
        note: "Counts hook calls.",
      },
      firstRequest: {
        cwd: PROJECT_WEB,
        model: "claude-haiku-5",
        measuredAt: "2026-11-14T10:00:00Z",
        total: 31_240,
        instructionTokens: 7910,
        badge: "Measured",
      },
      note: "Upstream's saved tokens cover tool-definition exposure only and are never added to spend",
    };
  }

  function summary(argv: string[]): CtlReply {
    if (options.summaryError) return fail("summary_failed", options.summaryError);
    const range = (flag(argv, "--range") ?? "7d") as RangeId;
    if (!(range in RANGE_DAYS))
      return fail("usage", `unknown range '${range}' (have: today, 7d, 30d)`);
    const project = flag(argv, "--project") ?? null;
    const known = PROJECTS.find(([key]) => key === project);
    if (project && !known) return fail("usage", `no project ${project}`);
    const scale = known ? known[1] : 1;
    const transcriptsOnly = phase !== "healthy" && phase !== "cannot-listen";
    const list = days(RANGE_DAYS[range], transcriptsOnly).map((d) => ({
      ...d,
      usd: d.usd * scale,
      estimated: d.estimated * scale,
    }));
    const usd = list.reduce((sum, d) => sum + d.usd, 0);
    const estimated = list.reduce((sum, d) => sum + d.estimated, 0);
    const sessions = Math.round(142 * scale * (RANGE_DAYS[range] / 7));
    const byDay: DayRow[] = list.map((d) => ({
      ...spendRow(
        d.key,
        d.usd,
        d.estimated,
        usd,
        Math.max(1, Math.round(sessions / list.length)),
        0,
      ),
      byModel: Object.fromEntries(
        MODEL_SPLIT.map(([model, share]) => [
          model,
          Math.round(d.usd * share * 1e6) / 1e6,
        ]),
      ),
    }));
    const totals = {
      ...spendRow("total", usd, estimated, usd, sessions, Math.round(sessions * 69)),
      key: "total",
    };
    const tile = tilesOf(phase, !showCost).map((t, index) => ({
      tokens: Math.round(t.tokens * scale),
      cost: cost(t.usd * scale, t.est * scale),
      costDelta: t.delta,
      previousUsd: Math.round(t.previous * scale * 100) / 100,
      estimatedDays: t.estDays,
      days: [1, 7, 30][index],
    }));
    const task = (key: string, share: number, n: number) =>
      spendRow(key, usd * share, key === "Interactive" ? estimated : 0, usd, n, 0);
    const out: SummaryData = {
      range,
      project,
      since: iso(Math.floor(SPEND_NOW / DAY) * DAY - (RANGE_DAYS[range] - 1) * DAY),
      totals,
      previous: {
        costDelta:
          tile[RANGE_DAYS[range] === 1 ? 0 : RANGE_DAYS[range] === 7 ? 1 : 2].costDelta,
        tokensDelta: 0.12,
      },
      cacheShare: { value: 0.958, badge: "Measured" },
      byDay,
      byProject: PROJECTS.filter(([key]) => !project || key === project).map(
        ([key, share, n]) =>
          spendRow(
            key,
            project ? usd : usd * share,
            project ? estimated : estimated * share,
            usd,
            n,
            Math.round(n * 69),
          ),
      ),
      byModel: MODEL_SPLIT.map(([key, share], index) =>
        spendRow(
          key,
          usd * share,
          estimated * share,
          usd,
          sessions,
          [3120, 6310, 440][index],
        ),
      ),
      byTask:
        phase === "healthy" || phase === "cannot-listen"
          ? TASKS.map(([key, share, n]) => task(key, share, n))
          : [task("Interactive", 1, sessions)],
      sources: {
        otelSessions: phase === "healthy" || phase === "cannot-listen" ? 112 : 0,
        transcriptSessions:
          phase === "healthy" || phase === "cannot-listen" ? 30 : sessions,
        estimatedShare: usd > 0 ? Math.round((estimated / usd) * 1000) / 1000 : 0,
      },
      tiles: { today: tile[0], "7d": tile[1], "30d": tile[2] },
      mcpPart: mcpPart(),
      stale:
        options.stale && phase === "healthy"
          ? {
              sessionsSince: 12,
              startedBeforeSetup: 3,
              projectOverrides: [
                `${FIXTURE_HOME}/work/clients/alpha/api/.claude/settings.json`,
              ],
            }
          : null,
      status: status(),
      notes: ["Spend is never added to upstream's saved-tokens number"],
    };
    if (project) {
      const list: SessionRow[] = [
        ["s-5", "2026-11-17T16:12:00Z", 14.2],
        ["s-4", "2026-11-16T10:40:00Z", 11.6],
        ["s-3", "2026-11-17T09:03:00Z", 2.1],
      ].map(([key, start, value]) => ({
        ...spendRow(String(key), Number(value), 0, usd, 1, 0),
        start: String(start),
        source: "otel" as const,
      }));
      out.sessionList = list;
    }
    return { ok: true, data: out };
  }

  function profiles(): string[] {
    return [PROFILE_PERSONAL, PROFILE_WORK];
  }

  function choose(argv: string[]): string[] | CtlReply {
    const named = flags(argv, "--profile");
    if (named.length === 0) return profiles();
    const unknown = named.find(
      (n) => !profiles().some((p) => p === n || p.startsWith(`${n}/`)),
    );
    if (unknown) return fail("usage", `no Claude Code profile '${unknown}'`);
    return profiles().filter((p) => named.some((n) => p === n || p.startsWith(`${n}/`)));
  }

  const KEYS: [string, (port: number) => string][] = [
    ["CLAUDE_CODE_ENABLE_TELEMETRY", () => "1"],
    ["OTEL_METRICS_EXPORTER", () => "otlp"],
    ["OTEL_LOGS_EXPORTER", () => "otlp"],
    ["OTEL_EXPORTER_OTLP_PROTOCOL", () => "http/json"],
    ["OTEL_EXPORTER_OTLP_ENDPOINT", (p) => `http://127.0.0.1:${p}`],
  ];

  function planProfile(path: string, wantPort: number, overwrite: string[]): PlanProfile {
    const entries: PlanEntry[] = KEYS.map(([key, value]) => {
      const wanted = value(wantPort);
      const conflict =
        options.conflict && path === PROFILE_PERSONAL && key === "OTEL_METRICS_EXPORTER";
      const current = conflict
        ? "prometheus"
        : setUp.has(path)
          ? key === "OTEL_EXPORTER_OTLP_ENDPOINT"
            ? `http://127.0.0.1:${port}`
            : value(wantPort)
          : null;
      const action: PlanEntry["action"] =
        current === null
          ? "add"
          : current === wanted
            ? "already set"
            : conflict
              ? overwrite.includes(key) || overwrite.includes("all")
                ? "overwrite"
                : "conflict"
              : "update";
      return { key, wanted, current, action };
    });
    return {
      profile: path,
      exists: true,
      entries,
      conflicts: entries.filter((e) => e.action === "conflict").length,
      writes: entries.filter((e) => ["add", "update", "overwrite"].includes(e.action))
        .length,
    };
  }

  function plan(argv: string[]): CtlReply {
    const chosen = choose(argv);
    if (!Array.isArray(chosen)) return chosen;
    const wantPort = Number(flag(argv, "--port") ?? port);
    if (wantPort < 1024) return fail("usage", "port must be 1024 or higher");
    const rows = chosen.map((p) => planProfile(p, wantPort, flags(argv, "--overwrite")));
    const conflicts = rows.reduce((sum, r) => sum + r.conflicts, 0);
    return {
      ok: true,
      data: {
        port: wantPort,
        endpoint: `http://127.0.0.1:${wantPort}`,
        profiles: rows,
        writes: rows.reduce((sum, r) => sum + r.writes, 0),
        conflicts,
        contentFlags: "never written",
        note:
          conflicts > 0
            ? "Keys that already hold another value stay as they are unless you choose to overwrite them; collection will not work until they match"
            : "Each profile's settings file is backed up by the settings writer before it changes",
      },
    };
  }

  function apply(argv: string[]): CtlReply {
    const planned = plan(argv);
    if (!planned.ok) return planned;
    const data = planned.data as {
      port: number;
      profiles: PlanProfile[];
      conflicts: number;
    };
    if (argv.includes("--dry-run"))
      return { ok: true, data: { dryRun: true, plan: planned.data } };
    for (const row of data.profiles) setUp.set(row.profile, iso(SPEND_NOW));
    port = data.port;
    dataDeleted = dataDeleted && false;
    if (phase === "never" || phase === "off" || phase === "cannot-listen")
      phase = "waiting";
    return {
      ok: true,
      data: {
        profiles: data.profiles.map((p) => ({ ...p, applied: p.writes > 0 })),
        port,
        enabled: true,
        complete: data.conflicts === 0,
      },
    };
  }

  function undo(argv: string[]): CtlReply {
    const named = flags(argv, "--profile");
    const targets = [...setUp.keys()].filter(
      (p) => named.length === 0 || named.some((n) => p === n || p.startsWith(`${n}/`)),
    );
    if (named.length > 0 && targets.length === 0)
      return fail("usage", `no Claude Code profile '${named[0]}'`);
    const remaining = setUp.size - targets.length;
    const deleteData = argv.includes("--delete-data");
    const stats = {
      events: hasData() ? 41206 : 0,
      bytes: hasData() ? Math.round(18.4 * MB) : 0,
      dailyLines: hasData() ? 62 : 0,
    };
    if (deleteData && !argv.includes("--dry-run") && !argv.includes("--yes"))
      return fail(
        "usage",
        "--delete-data deletes the collected events and daily totals for good: add --yes, or --dry-run to look first",
      );
    if (deleteData && remaining > 0)
      return fail(
        "usage",
        "--delete-data needs every profile undone: drop --profile or undo the rest first",
      );
    if (argv.includes("--dry-run"))
      return {
        ok: true,
        data: {
          dryRun: true,
          profilesSetUp: setUp.size,
          remaining,
          ...(deleteData ? { deleteData: stats } : {}),
        },
      };
    const profilesDone = targets.map((profile) => ({
      profile,
      restored: KEYS.map(([key]) => key),
      keptChanged: [] as string[],
    }));
    for (const profile of targets) setUp.delete(profile);
    if (remaining === 0) {
      phase = hasData() || files > 0 ? "off" : "never";
      if (deleteData) dataDeleted = true;
    }
    return {
      ok: true,
      data: {
        profiles: profilesDone,
        enabled: remaining > 0,
        ...(deleteData ? { deleted: stats } : {}),
      },
    };
  }

  function prune(argv: string[]): CtlReply {
    const dry = argv.includes("--dry-run");
    if (argv.includes("--totals") && !dry && !argv.includes("--yes"))
      return fail(
        "usage",
        "--totals deletes the daily totals for good: add --yes, or --dry-run to look first",
      );
    const data: PruneData = {
      dryRun: dry,
      removedEvents: dry ? 5120 : 0,
      removedBytes: dry ? 2 * MB : 0,
      keptEvents: 41206,
      keptBytes: Math.round(18.4 * MB),
      foldedDays: dry ? 2 : 0,
      retentionDays,
      maxBytes: maxMb * MB,
      totalsDeleted: argv.includes("--totals") && !dry,
    };
    return { ok: true, data };
  }

  function config(argv: string[]): CtlReply {
    const next = {
      port: Number(flag(argv, "--port") ?? port),
      retentionDays: Number(flag(argv, "--retention-days") ?? retentionDays),
      maxMb: Number(flag(argv, "--max-mb") ?? maxMb),
    };
    if (next.port < 1024) return fail("usage", "port must be 1024 or higher");
    if (next.retentionDays < 7 || next.retentionDays > 90)
      return fail("usage", "retention must be 7 to 90 days");
    if (next.maxMb < 16 || next.maxMb > 256)
      return fail("usage", "size cap must be 16 to 256 MB");
    const cost = flag(argv, "--show-cost");
    const portChanged = next.port !== port;
    const changed =
      portChanged ||
      next.retentionDays !== retentionDays ||
      next.maxMb !== maxMb ||
      (cost !== undefined && (cost === "on") !== showCost);
    port = next.port;
    retentionDays = next.retentionDays;
    maxMb = next.maxMb;
    if (cost !== undefined) showCost = cost === "on";
    const data: ConfigSetData = {
      port,
      retentionDays,
      maxMb,
      showCost,
      enabled: phase !== "never" && phase !== "off",
      portChanged,
      changed,
    };
    return { ok: true, data };
  }

  function exportCsv(argv: string[]): CtlReply {
    const out = flag(argv, "--out");
    const csv =
      "day,project,model,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,cost_usd,cost_basis,requests,source\n" +
      `2026-11-18,${PROJECT_WEB},claude-opus-5,1000,200,8000,500,0.120000,Measured,3,otel\n`;
    return out
      ? { ok: true, data: { rows: 1, out, bytes: csv.length } }
      : { ok: true, data: { rows: 1, csv } };
  }

  return {
    get phase() {
      return phase;
    },
    get port() {
      return port;
    },
    get showCost() {
      return showCost;
    },
    run(argv: string[]): CtlReply {
      const [, verb, sub] = argv;
      if (verb === "status") return { ok: true, data: status() };
      if (verb === "receiver") return { ok: true, data: status() };
      if (verb === "summary") {
        const answer = summary(argv);
        if (scanned) return answer;
        const wait = options.scanMs ?? 4000;
        scanTimer ??= setTimeout(() => {
          scanned = true;
        }, wait);
        return { ...answer, delayMs: wait };
      }
      if (verb === "setup" && sub === "plan") return plan(argv);
      if (verb === "setup" && sub === "apply") return apply(argv);
      if (verb === "setup" && sub === "undo") return undo(argv);
      if (verb === "prune") return prune(argv);
      if (verb === "config") return config(argv);
      if (verb === "export-csv") return exportCsv(argv);
      return fail("usage", `unknown spend command: ${argv.slice(1).join(" ")}`);
    },
  };
}

export type SpendWorld = ReturnType<typeof createSpendWorld>;
