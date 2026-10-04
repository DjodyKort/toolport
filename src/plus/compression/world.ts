/** A stateful Compression world: the same argv rows the tab runs, but an applied write changes
 * what the next read answers (a provider switch changes the status strip and the doctor, an
 * install changes the engine and the pin, a recorded entry grows the ledger). A preview
 * (`--dry-run`) never changes anything. Browser safe: JSON imports only, no node modules. */
import { ctlReplyFailure } from "../fixtures/ctlReply";
import disablePreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-disable.preview.json";
import presets from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-presets.json";
import sealPreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-seal.preview.json";
import setProviderApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-set-provider.apply.json";
import setProviderPreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-set-provider.preview.json";
import status from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-status.json";
import verify from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-verify.measured.json";

type Golden = { envelope: { data: unknown } };
type Loose = Record<string, unknown>;

const FIXTURE = "/fixture";
const scrub = <T>(value: T): T =>
  JSON.parse(JSON.stringify(value).split("<WORLD>").join(FIXTURE)) as T;
const data = (golden: unknown) => scrub((golden as Golden).envelope.data) as Loose;

const DATA = `${FIXTURE}/data`;
const HOME = `${FIXTURE}/home`;
const STAMP = "2026-10-04T10:00:00Z";
const LATEST = "0.30.0";
const VERSION = /^\d+(\.\d+){1,3}([-+.][0-9A-Za-z.]+)?$/;
const SHIM_FUNCTIONS = 4;

export const PROVIDER_IDS = ["rtk-only", "headroom", "parsec", "none"] as const;
export type Provider = (typeof PROVIDER_IDS)[number];
const RUNTIME: Record<Provider, string> = {
  headroom: "proxy",
  "rtk-only": "hook",
  parsec: "plugin",
  none: "none",
};

const PRESETS = (data(presets).presets as Loose[]).map((p) => ({
  name: p.name as string,
  mode: p.mode as string,
  port: p.port as number,
  savingsProfile: p.savingsProfile as string | null,
}));
const KNOB: Record<string, string> = { agent: "50", balanced: "80" };
const SEAL_KNOBS = data(sealPreview).declarable as Array<{ knob: string; value: string }>;
const SEAL_UNSET = data(sealPreview).unset as string[];
const ENGINE_ACTIONS = {
  preview: (data(setProviderPreview).actions as string[]).slice(1),
  apply: (data(setProviderApply).actions as string[]).slice(1),
};
const ENGINE_WARNINGS = data(setProviderApply).warnings as string[];
const TEARDOWN_PREVIEW = (data(disablePreview).actions as string[]).slice(1);

export interface Entry {
  provider: string;
  before: number;
  after: number;
}

export interface WorldState {
  provider: Provider;
  preset: string;
  mode: string | null;
  port: number | null;
  pin: string;
  installed: string | null;
  snapshot: string | null;
  shims: boolean;
  proxy: boolean;
  sealed: boolean;
  entries: Entry[];
}

const requirement = (pin: string) => `headroom-ai[proxy,code,ml]==${pin}`;
const flag = (argv: string[], name: string) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv[at + 1] : undefined;
};
const fail = (code: string, message: string, extra?: unknown) =>
  ctlReplyFailure(code, message, extra);

/** Today's state of a Mac: rtk-only on the hook runtime, no engine installed, an empty ledger. */
export function createCompressionWorld(initial: Partial<WorldState> = {}) {
  const s: WorldState = {
    provider: "rtk-only",
    preset: "interactive",
    mode: null,
    port: null,
    pin: "0.29.0",
    installed: null,
    snapshot: null,
    shims: false,
    proxy: false,
    sealed: false,
    entries: [],
    ...initial,
  };

  const base = (name: string) => PRESETS.find((p) => p.name === name)!;
  const presetOf = (t: WorldState, name: string) => {
    const p = base(name);
    const active = name === t.preset;
    return {
      mode: active && t.mode ? t.mode : p.mode,
      name,
      port: active && t.port ? t.port : p.port,
      savingsProfile: p.savingsProfile,
    };
  };
  const portOf = (t: WorldState = s) => presetOf(t, t.preset).port;
  const knobCount = (name: string) => (base(name).savingsProfile && s.snapshot ? 1 : 0);
  const snapshotOf = (name: string) => (base(name).savingsProfile ? s.snapshot : null);
  const headroom = () => s.provider === "headroom";

  const statusData = () => {
    const golden = data(status);
    return {
      ...golden,
      configExists: true,
      provider: s.provider,
      runtime: RUNTIME[s.provider],
      preset: {
        ...presetOf(s, s.preset),
        knobCount: knobCount(s.preset),
        snapshotVersion: snapshotOf(s.preset),
      },
      pin: {
        package: "headroom-ai",
        pin: s.pin,
        requirement: requirement(s.pin),
        installed: headroom() ? s.installed : null,
        drift: headroom() ? s.installed !== s.pin : null,
      },
      shims: { ...(golden.shims as object), exists: s.shims },
    };
  };

  const presetsData = () => ({
    active: s.preset,
    presets: PRESETS.map((p) => ({
      active: p.name === s.preset,
      ...presetOf(s, p.name),
      knobCount: knobCount(p.name),
      snapshotVersion: snapshotOf(p.name),
    })),
  });

  const pinData = () => ({
    adopted: null,
    drift: s.installed !== null && s.installed !== s.pin,
    dryRun: false,
    install: null,
    installed: s.installed,
    pin: s.pin,
    refresh: null,
    requirement: requirement(s.pin),
    set: false,
  });

  const refresh = (dry: boolean) => {
    if (s.installed === null)
      return fail("snapshot_failed", "preset 'agent': headroom not on PATH");
    const changed = s.snapshot !== s.installed;
    const out = {
      version: s.installed,
      presets: PRESETS.filter((p) => p.savingsProfile).map((p) => ({
        name: p.name,
        changed,
        added: changed ? [{ knob: "HEADROOM_MAX_ITEMS", value: KNOB[p.name] }] : [],
        removed: [],
        moved: [],
        kept: [],
      })),
    };
    if (!dry) s.snapshot = s.installed;
    return out;
  };

  const writeData = (t: WorldState, dry: boolean, extra: Loose = {}) => {
    const engine = t.provider === "headroom";
    const teardown = extra.teardown === true;
    const save = `${dry ? "would save" : "saved"} config (provider=${t.provider})`;
    const down = teardown
      ? dry
        ? TEARDOWN_PREVIEW
        : s.installed
          ? TEARDOWN_PREVIEW.map((line) => line.replace("would run", "ran"))
          : []
      : [];
    return {
      actions: [
        save,
        ...(engine ? ENGINE_ACTIONS[dry ? "preview" : "apply"] : []),
        ...down,
      ],
      adopted: null,
      dryRun: dry,
      preset: presetOf(t, t.preset),
      provider: t.provider,
      removed: [],
      runtime: RUNTIME[t.provider],
      warnings:
        engine && t.installed === null
          ? ENGINE_WARNINGS
          : teardown && !dry && !s.installed
            ? ["headroom mcp uninstall: headroom not on PATH"]
            : [],
      written: engine
        ? [`${DATA}/compression-env.sh`, `${DATA}/compression-shims.zsh`]
        : [],
      ...extra,
    };
  };

  /** A policy write: the change is made on a copy and only kept when it is applied. */
  const change = (dry: boolean, mutate: (t: WorldState) => unknown, extra?: Loose) => {
    const t = { ...s };
    const failed = mutate(t);
    if (failed) return failed;
    if (!dry) {
      if (t.provider === "headroom") t.shims = true;
      Object.assign(s, t);
    }
    return writeData(t, dry, extra);
  };

  const known = (name: string | undefined) =>
    PRESETS.some((p) => p.name === name) ? null : fail("usage", `unknown preset ${name}`);
  const provider = (name: string | undefined) =>
    (PROVIDER_IDS as readonly string[]).includes(name ?? "")
      ? null
      : fail("usage", `unknown provider ${name}`);

  const check = (name: string, ok: boolean, detail: string) => ({ name, ok, detail });
  const checks = () => {
    const out = [];
    const port = portOf();
    const ready = headroom() && s.installed !== null && s.proxy;
    const binary = s.provider === "rtk-only" ? "rtk" : s.provider;
    if (s.provider === "none")
      out.push(check("engine", true, "compression disabled (provider none)"));
    else if (s.provider === "headroom") {
      const have = s.installed;
      out.push(
        check(
          "engine binary",
          have !== null,
          have === null
            ? `headroom not on PATH (pin ${s.pin})`
            : have === s.pin
              ? `${have} == pin ${s.pin}`
              : `${have} != pin ${s.pin}`,
        ),
      );
      if (have !== null)
        out.push(
          check(
            "engine version",
            have === s.pin,
            have === s.pin
              ? `${have} == pin`
              : `${have} != pin ${s.pin}: unverified build, run \`compression update\``,
          ),
        );
      out.push(
        check(
          "engine reachable",
          ready,
          ready ? `proxy ready on :${port}` : `no ready proxy on :${port}`,
        ),
      );
    } else
      out.push(
        check("engine", true, `${binary} binary found at ${FIXTURE}/bin/${binary}`),
      );
    if (!headroom()) out.push(check("pin", true, `not used by provider ${s.provider}`));
    else {
      out.push(check("pin", true, `exact pin ${requirement(s.pin)}`));
      const stale = PRESETS.filter((p) => p.savingsProfile && s.snapshot !== s.pin);
      out.push(
        check(
          "preset provenance",
          stale.length === 0,
          stale.length === 0
            ? "all snapshots match the pin"
            : `snapshotted from another build: ${stale.map((p) => p.name).join(", ")}`,
        ),
      );
    }
    const shim = `${DATA}/compression-shims.zsh`;
    out.push(
      s.shims
        ? check("shims", true, `${shim} defines all ${SHIM_FUNCTIONS} functions`)
        : check(
            "shims",
            !headroom(),
            headroom() ? `${shim} missing` : "not required for this provider",
          ),
    );
    if (ready) {
      out.push(
        check(
          "engine fingerprint",
          s.installed === s.pin,
          s.installed === s.pin
            ? `running ${s.installed} == pin, digest 3fa9c2d1, contract keys present`
            : `running ${s.installed} != pin ${s.pin} (digest 3fa9c2d1)`,
        ),
        check(
          "sealed posture",
          s.sealed,
          `${
            s.sealed
              ? "every declarable knob is policy"
              : `vendor-decided, declarable: ${SEAL_KNOBS.map((k) => `${k.knob}=${k.value}`).join(", ")}`
          }; ${SEAL_UNSET.length} at the engine's internal default (pinned)`,
        ),
      );
    }
    return out;
  };

  const unhealthy = (body: Loose) =>
    (body.checks as Array<{ ok: boolean }>).every((c) => c.ok)
      ? body
      : fail("unhealthy", "one or more checks failed", body);

  const summary = () => {
    const by = new Map<string, Entry[]>();
    for (const entry of s.entries)
      by.set(entry.provider, [...(by.get(entry.provider) ?? []), entry]);
    const providers = [...by.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([name, rows]) => {
        const before = rows.reduce((n, r) => n + r.before, 0);
        const after = rows.reduce((n, r) => n + r.after, 0);
        return {
          provider: name,
          launches: 0,
          routed: 0,
          plain: 0,
          savingsEntries: rows.length,
          tokensBefore: before,
          tokensAfter: after,
          tokensSaved: before - after,
          savedPercent:
            before > 0 ? Math.round(((before - after) / before) * 1000) / 10 : null,
        };
      });
    return {
      launchesPath: `${DATA}/compression-launches.jsonl`,
      providers,
      savingsPath: `${DATA}/compression-savings.jsonl`,
      tokensSaved: providers.reduce((n, p) => n + p.tokensSaved, 0),
    };
  };

  const record = (rest: string[]) => {
    const name = flag(rest, "--provider");
    const before = flag(rest, "--before");
    const after = flag(rest, "--after");
    if (provider(name)) return provider(name);
    if (!/^\d+$/.test(before ?? "") || !/^\d+$/.test(after ?? ""))
      return fail("usage", "--before and --after are required whole numbers");
    s.entries.push({ provider: name!, before: Number(before), after: Number(after) });
    return {
      path: `${DATA}/compression-savings.jsonl`,
      recorded: {
        provider: name,
        session: flag(rest, "--session") ?? null,
        source: flag(rest, "--source") ?? "",
        tokens_after: Number(after),
        tokens_before: Number(before),
        ts: STAMP,
      },
      tokensSaved: Number(before) - Number(after),
    };
  };

  const proxy = (sub: string) => {
    const port = portOf();
    const steps: string[] = [];
    if (sub !== "up") {
      if (s.proxy) {
        s.proxy = false;
        steps.push(`stopped proxy on :${port} (4242)`);
      } else if (sub === "down")
        return fail("proxy_down", `no proxy listening on :${port}`);
      else steps.push(`no proxy listening on :${port}`);
    }
    if (sub !== "down") {
      if (s.installed === null) return fail("proxy_up", "headroom not on PATH");
      const mode = presetOf(s, s.preset).mode;
      steps.push(
        s.proxy
          ? `reusing proxy on :${port}`
          : `started proxy on :${port} (mode=${mode})`,
      );
      s.proxy = true;
    }
    return { action: sub, port, steps };
  };

  const seal = (dry: boolean) => {
    const port = portOf();
    if (!s.proxy)
      return fail(
        "no_proxy",
        `no proxy on :${port}: start one first (toolportctl compression proxy up). /health is the only truthful source for what a build actually runs.`,
      );
    const declarable = s.sealed ? [] : SEAL_KNOBS;
    const out = {
      apply: !dry,
      complete: false,
      declarable,
      dryRun: dry,
      port,
      preset: s.preset,
      sealed: dry ? 0 : declarable.length,
      unset: SEAL_UNSET,
      version: s.installed,
    };
    if (!dry) s.sealed = true;
    return out;
  };

  const verifyCmd = (rest: string[]) => {
    const folder = flag(rest, "--transcripts");
    const golden = data(verify);
    const body = folder
      ? {
          buckets: null,
          checks: checks(),
          provider: s.provider,
          transcripts: { count: 0, root: folder },
        }
      : { ...golden, checks: checks(), provider: s.provider };
    return folder
      ? fail("unhealthy", "one or more checks failed", body)
      : unhealthy(body);
  };

  const launch = (rest: string[]) => {
    const cwd = flag(rest, "--cwd") ?? HOME;
    const preset = presetOf(s, s.preset);
    const routed = headroom() && s.installed === s.pin;
    const refused = headroom() && !routed;
    const set = {
      ANTHROPIC_BASE_URL: `http://127.0.0.1:${preset.port}`,
      HEADROOM_MODE: preset.mode,
    };
    return { cwd, preset, routed, refused, set };
  };

  const run = (rest: string[]) => {
    const { cwd, preset, routed, refused, set } = launch(rest);
    return {
      argv: ["claude"],
      cwd,
      env: routed ? { set, unset: [] } : { set: {}, unset: ["ANTHROPIC_BASE_URL"] },
      installed: headroom() ? s.installed : null,
      ledger: {
        cwd,
        pin: s.pin,
        port: routed ? preset.port : null,
        preset: preset.name,
        provider: s.provider,
        routed,
      },
      pin: s.pin,
      preset: preset.name,
      program: "claude",
      provider: s.provider,
      proxy: routed
        ? {
            argv: [
              "headroom",
              "proxy",
              "--port",
              String(preset.port),
              "--mode",
              preset.mode,
            ],
            mode: preset.mode,
            port: preset.port,
          }
        : null,
      routed,
      warnings: refused
        ? [
            s.installed === null
              ? `headroom not on PATH (pin ${s.pin}); launching plain claude`
              : `installed ${s.installed} differs from the pin ${s.pin}; launching plain claude`,
          ]
        : [],
    };
  };

  const env = (rest: string[]) => {
    const { cwd, preset, routed, set } = launch(rest);
    if (!headroom())
      return {
        cwd,
        env: {},
        launch: "plain",
        lines: ["HRCOMPRESS_LAUNCH=plain"],
        port: null,
        preset: preset.name,
        provider: s.provider,
      };
    return {
      cwd,
      env: set,
      launch: routed ? "route" : "plain",
      lines: [
        ...Object.entries(set).map(([k, v]) => `export ${k}="${v}"`),
        `HRCOMPRESS_PORT=${preset.port}`,
        `HRCOMPRESS_PRESET=${preset.name}`,
        "HRCOMPRESS_LAUNCH=route",
      ],
      port: preset.port,
      preset: preset.name,
      provider: s.provider,
    };
  };

  const pinCmd = (rest: string[], dry: boolean) => {
    const version = rest.find((a) => !a.startsWith("--"));
    if (version) {
      if (!VERSION.test(version))
        return fail("usage", `'${version}' is not a parseable X.Y.Z version`);
      const out = {
        ...pinData(),
        drift: s.installed !== null && s.installed !== version,
        dryRun: dry,
        pin: version,
        requirement: requirement(version),
        set: true,
      };
      if (!dry) s.pin = version;
      return out;
    }
    if (rest.includes("--install")) {
      const need = requirement(s.pin);
      if (dry)
        return {
          ...pinData(),
          dryRun: true,
          install: { requirement: need, dryRun: true },
          restartProxies: false,
        };
      const restart = s.proxy;
      s.installed = s.pin;
      return {
        ...pinData(),
        install: {
          requirement: need,
          dryRun: false,
          detail: `installed headroom ${s.pin}`,
        },
        restartProxies: restart,
      };
    }
    if (rest.includes("--refresh")) {
      const out = refresh(dry);
      return "code" in out ? out : { ...pinData(), dryRun: dry, refresh: out };
    }
    return pinData();
  };

  const update = (rest: string[]) => {
    const target = flag(rest, "--to") ?? (rest.includes("--latest") ? LATEST : undefined);
    if (!target || !VERSION.test(target))
      return fail("usage", "update needs --latest or --to <version>");
    const accept = rest.includes("--accept");
    const out = { accepted: accept, current: s.pin, same: target === s.pin, target };
    if (accept && !out.same) {
      s.pin = target;
      s.installed = target;
      s.snapshot = target;
    }
    return out;
  };

  function reply(argv: string[]): unknown {
    if (argv[0] !== "compression") return undefined;
    const [, sub, ...rest] = argv;
    const dry = rest.includes("--dry-run");
    switch (sub) {
      case "status":
        return statusData();
      case "presets": {
        if (!rest.includes("--refresh")) return presetsData();
        const out = refresh(dry);
        return "code" in out ? out : { ...presetsData(), refresh: out };
      }
      case "pin":
        return pinCmd(rest, dry);
      case "doctor": {
        const list = checks();
        return unhealthy({
          checks: list,
          healthy: list.every((c) => c.ok),
          migrated: [],
          provider: s.provider,
        });
      }
      case "verify":
        return verifyCmd(rest);
      case "set-provider":
        return change(
          dry,
          (t) => provider(rest[0]) ?? void (t.provider = rest[0] as Provider),
        );
      case "use":
        return change(dry, (t) => {
          const failed = known(rest[0]);
          if (failed) return failed;
          Object.assign(t, { preset: rest[0], mode: null, port: null });
        });
      case "enable": {
        const wanted =
          flag(rest, "--provider") ?? (s.provider === "none" ? "headroom" : s.provider);
        return change(
          dry,
          (t) => {
            const failed =
              provider(wanted) ??
              (wanted === "none"
                ? fail("usage", "use disable to turn compression off")
                : null);
            if (failed) return failed;
            t.provider = wanted as Provider;
            const name = flag(rest, "--preset");
            if (name) {
              const unknown = known(name);
              if (unknown) return unknown;
              Object.assign(t, { preset: name, mode: null, port: null });
            }
            const mode = flag(rest, "--mode");
            const port = flag(rest, "--port");
            if (mode) t.mode = mode;
            if (port) t.port = Number(port);
          },
          {
            nextSteps: [
              "verify: toolportctl compression status / doctor   (MCP entry already registered)",
            ],
          },
        );
      }
      case "disable":
        return change(dry, (t) => void (t.provider = "none"), {
          teardown: rest.includes("--teardown"),
        });
      case "sync":
        return change(dry, () => null);
      case "update":
        return update(rest);
      case "seal":
        return seal(rest.includes("--dry-run"));
      case "proxy":
        return ["up", "down", "restart"].includes(rest[0]) ? proxy(rest[0]) : undefined;
      case "ledger":
        return rest[0] === "summary"
          ? summary()
          : rest[0] === "record"
            ? record(rest.slice(1))
            : undefined;
      case "run":
        return rest.includes("--plan") ? run(rest) : undefined;
      case "env":
        return env(rest);
      default:
        return undefined;
    }
  }

  return { state: s, reply };
}

export type CompressionWorld = ReturnType<typeof createCompressionWorld>;
