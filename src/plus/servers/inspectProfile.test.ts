import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

import { serversWorld } from "../fixtures/servers";
import { inspectProfile, summarize, type InspectRun } from "./inspectProfile";
import { createBridge, ctlFailure, type Bridge } from "./testkit";

const [def, work, research] = serversWorld.profileLs.profiles;
let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  mocks.listen.mockReset().mockResolvedValue(() => {});
  mocks.invoke.mockReset().mockImplementation(bridge.invoke);
});

function run(profile = work) {
  const updates: InspectRun[] = [];
  const controller = new AbortController();
  const result = inspectProfile(profile, {
    signal: controller.signal,
    onUpdate: (update) => updates.push(update),
  });
  return { result, updates, controller };
}

const stateOf = (result: InspectRun) =>
  Object.fromEntries(result.rows.map((row) => [row.name, row.state]));

describe("inspectProfile", () => {
  it("uses the whole-profile command when every server answers", async () => {
    const { result, updates } = run(research);
    const done = await result;
    expect(stateOf(done)).toEqual({
      "docs-search": "ok",
      "wiki-reader": "ok",
      "scratch-notes": "ok",
    });
    expect(done).toMatchObject({ done: true, stoppedAt: null });
    expect(summarize(done.rows)).toEqual({
      ok: 3,
      login: 0,
      failed: 0,
      pending: 0,
      tools: 6,
    });
    expect(bridge.ran()).toEqual(["profile inspect research"]);
    expect(updates[0].rows.every((row) => row.state === "pending")).toBe(true);
  });

  it("asks each server on its own when the whole-profile command stops at a 401", async () => {
    const { result } = run(work);
    const done = await result;
    expect(done.stoppedAt).toBe("issue-tracker: HTTP 401 invalid_token");
    expect(stateOf(done)).toEqual({
      "corp-tools": "ok",
      "issue-tracker": "login",
      "wiki-reader": "ok",
    });
    expect(done.done).toBe(true);
    expect(summarize(done.rows)).toEqual({
      ok: 2,
      login: 1,
      failed: 0,
      pending: 0,
      tools: 4,
    });
    expect(done.rows.find((row) => row.state === "login")?.message).toMatch(/401/);
    expect(bridge.ran().sort()).toEqual([
      "inspect srv-corp",
      "inspect srv-issues",
      "inspect srv-wiki",
      "profile inspect work",
    ]);
  });

  it("separates a server that needs a login from one that cannot start", async () => {
    const { result } = run(def);
    const done = await result;
    expect(stateOf(done)).toEqual({
      "docs-search": "ok",
      "corp-tools": "ok",
      "wiki-reader": "ok",
      "mail-bridge": "ok",
      "issue-tracker": "login",
      "design-files": "login",
      "acme-erp": "failed",
    });
    expect(done.rows.find((row) => row.name === "acme-erp")?.message).toMatch(
      /could not start/,
    );
  });

  it("reports the rows as they come in, so the dialog fills while servers connect", async () => {
    const { result, updates } = run(work);
    await result;
    const pendingCounts = updates.map((update) => summarize(update.rows).pending);
    expect(pendingCounts[0]).toBe(3);
    expect(pendingCounts.at(-1)).toBe(0);
    expect(updates.at(-1)?.done).toBe(true);
    expect(
      updates.some((update) => !update.done && summarize(update.rows).pending > 0),
    ).toBe(true);
  });

  it("says a server is missing when the command does not report it", async () => {
    bridge.set("profile inspect research", {
      profile: "research",
      servers: [{ id: "srv-docs", tools: [] }],
    });
    const done = await run(research).result;
    expect(stateOf(done)).toEqual({
      "docs-search": "ok",
      "wiki-reader": "failed",
      "scratch-notes": "failed",
    });
    expect(done.rows[1].message).toBe("The command did not report this server");
  });

  it("finishes at once for a profile without servers", async () => {
    const done = await run({ ...work, servers: [] }).result;
    expect(done).toMatchObject({ rows: [], done: true });
    expect(bridge.ran()).toEqual([]);
  });

  it("stops asking once it is cancelled and is not done", async () => {
    bridge.set(
      "profile inspect work",
      ctlFailure("inspect_failed", "issue-tracker: HTTP 401"),
    );
    const { result, controller } = run(work);
    controller.abort();
    const done = await result;
    expect(done.done).toBe(false);
    expect(bridge.cancelled.length).toBeGreaterThan(0);
  });
});
