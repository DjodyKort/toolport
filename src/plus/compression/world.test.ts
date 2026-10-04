import { describe, expect, it } from "vitest";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { ctlShapes } from "../bridge/data";
import { check } from "../bridge/shape";
import { compressionShapes } from "../types/compression";
import { createCompressionWorld } from "./world";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Answer = Record<string, any>;
const ask = (world: ReturnType<typeof createCompressionWorld>, line: string) =>
  world.reply(`compression ${line}`.split(" ")) as Answer;
const failed = (value: unknown) => {
  expect(value).toBeInstanceOf(CtlReplyFailure);
  return value as CtlReplyFailure;
};

describe("compression world", () => {
  it("starts as today's Mac: rtk-only on the hook runtime, nothing installed", () => {
    const world = createCompressionWorld();
    const status = ask(world, "status");
    expect([status.provider, status.runtime, status.preset.name]).toEqual([
      "rtk-only",
      "hook",
      "interactive",
    ]);
    expect(status.pin.installed).toBeNull();
    expect(ask(world, "ledger summary").providers).toEqual([]);
  });

  it("keeps the real shapes of the reads and the writes", () => {
    const world = createCompressionWorld({
      provider: "headroom",
      installed: "0.29.0",
      proxy: true,
    });
    const shapes = { ...ctlShapes, ...compressionShapes };
    for (const [line, stem] of [
      ["status", "compression-status"],
      ["presets", "compression-presets"],
      ["pin", "compression-pin"],
      ["set-provider none --dry-run", "compression-set-provider.preview"],
      ["sync", "compression-sync.apply"],
      ["use agent", "compression-use.apply"],
      ["seal --dry-run", "compression-seal.preview"],
      ["update --latest", "compression-update.preview"],
      [
        "ledger record --provider none --before 5 --after 3",
        "compression-ledger-record.apply",
      ],
    ])
      expect(check(shapes[stem], ask(world, line)), line).toEqual([]);
  });

  it("measures a healthy provider and refuses a folder without transcripts", () => {
    const world = createCompressionWorld();
    const shapes = { ...ctlShapes, ...compressionShapes };
    expect(check(shapes["compression-verify.measured"], ask(world, "verify"))).toEqual(
      [],
    );
    const none = failed(ask(world, "verify --transcripts /fixture/none"));
    expect(none.code).toBe("unhealthy");
    expect(none.data).toMatchObject({ buckets: null, transcripts: { count: 0 } });
  });

  it("changes nothing on a preview and changes the next read on an apply", () => {
    const world = createCompressionWorld();
    ask(world, "set-provider headroom --dry-run");
    expect(ask(world, "status").provider).toBe("rtk-only");
    ask(world, "set-provider headroom");
    const status = ask(world, "status");
    expect([status.provider, status.runtime]).toEqual(["headroom", "proxy"]);
    expect(status.shims.exists).toBe(true);
    ask(world, "use agent");
    expect(ask(world, "presets").active).toBe("agent");
    ask(world, "disable");
    expect(ask(world, "status").provider).toBe("none");
  });

  it("needs the engine for the proxy and a proxy for the seal", () => {
    const world = createCompressionWorld({ provider: "headroom" });
    expect(failed(ask(world, "proxy up")).code).toBe("proxy_up");
    expect(failed(ask(world, "proxy down")).code).toBe("proxy_down");
    expect(failed(ask(world, "seal --dry-run")).code).toBe("no_proxy");
    ask(world, "pin --install --dry-run");
    expect(ask(world, "pin").installed).toBeNull();
    ask(world, "pin --install");
    expect(ask(world, "pin").installed).toBe("0.29.0");
    expect(ask(world, "proxy up").steps[0]).toMatch(/^started proxy on :8787/);
    expect(ask(world, "seal --apply").sealed).toBe(2);
    expect(ask(world, "seal --apply").sealed).toBe(0);
    ask(world, "proxy down");
    expect(failed(ask(world, "seal --apply")).code).toBe("no_proxy");
  });

  it("reports the unhealthy checks of a provider without an engine, with data", () => {
    const world = createCompressionWorld({ provider: "headroom" });
    const down = failed(ask(world, "doctor"));
    expect(down.code).toBe("unhealthy");
    const names = (down.data as { checks: Array<{ name: string; ok: boolean }> }).checks
      .filter((c) => !c.ok)
      .map((c) => c.name);
    expect(names).toEqual([
      "engine binary",
      "engine reachable",
      "preset provenance",
      "shims",
    ]);
    ask(world, "set-provider headroom");
    ask(world, "pin --install");
    ask(world, "pin --refresh");
    ask(world, "proxy up");
    ask(world, "seal --apply");
    expect(ask(world, "doctor").healthy).toBe(true);
  });

  it("moves the pin and the install together on an accepted update", () => {
    const world = createCompressionWorld();
    expect(ask(world, "update --latest")).toMatchObject({
      current: "0.29.0",
      target: "0.30.0",
      same: false,
    });
    expect(ask(world, "pin").pin).toBe("0.29.0");
    ask(world, "update --latest --accept");
    expect(ask(world, "pin")).toMatchObject({
      pin: "0.30.0",
      installed: "0.30.0",
      drift: false,
    });
    expect(ask(world, "update --latest").same).toBe(true);
  });

  it("grows the ledger from the recorded entries and keeps the totals true", () => {
    const world = createCompressionWorld();
    ask(world, "ledger record --provider rtk-only --before 1000 --after 400");
    ask(world, "ledger record --provider headroom --before 20000 --after 8000");
    ask(world, "ledger record --provider headroom --before 1000 --after 1200");
    const ledger = ask(world, "ledger summary");
    expect(ledger.providers.map((p: { provider: string }) => p.provider)).toEqual([
      "headroom",
      "rtk-only",
    ]);
    expect(ledger.providers[0]).toMatchObject({
      savingsEntries: 2,
      tokensBefore: 21000,
      tokensAfter: 9200,
      tokensSaved: 11800,
    });
    expect(ledger.providers[1]).toMatchObject({ tokensSaved: 600, savedPercent: 60 });
    expect(ledger.tokensSaved).toBe(12400);
    expect(
      failed(ask(world, "ledger record --provider nope --before 1 --after 1")).code,
    ).toBe("usage");
  });

  it("plans a launch by the policy: plain without an engine, routed with the pinned one", () => {
    const world = createCompressionWorld({ provider: "headroom" });
    expect(ask(world, "run --plan --cwd /fixture/p claude")).toMatchObject({
      routed: false,
      cwd: "/fixture/p",
    });
    expect(ask(world, "run --plan --cwd /fixture/p claude").warnings).toHaveLength(1);
    ask(world, "pin --install");
    const plan = ask(world, "run --plan --cwd /fixture/p claude");
    expect(plan).toMatchObject({ routed: true, proxy: { port: 8787 } });
    expect(ask(world, "env --cwd /fixture/p")).toMatchObject({
      launch: "route",
      port: 8787,
    });
  });

  it("does not answer an argv it does not know", () => {
    expect(createCompressionWorld().reply(["compression", "frobnicate"])).toBeUndefined();
    expect(createCompressionWorld().reply(["skills", "ls"])).toBeUndefined();
  });
});
