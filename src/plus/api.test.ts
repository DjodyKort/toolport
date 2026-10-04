import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthFixAction } from "./api";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { fixIsActionable, plusAuthFix, plusAuthNotifications, plusAuthRows, plusInvoke } =
  await import("./api");

function fix(over: Partial<AuthFixAction>): AuthFixAction {
  return {
    action: "reauth",
    server: "figma",
    label: "Sign in to figma again",
    command: "toolportctl auth login figma",
    ipc: null,
    ...over,
  };
}

describe("plus api", () => {
  beforeEach(() => invoke.mockReset());

  it("routes every command through plus_invoke", async () => {
    invoke.mockResolvedValue({ ok: true });
    await expect(plusInvoke("plus.x", { a: 1 })).resolves.toEqual({ ok: true });
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.x",
      args: { a: 1 },
    });
  });

  it("reads the auth rows with empty args", async () => {
    invoke.mockResolvedValue({ counts: {}, rows: [] });
    await expect(plusAuthRows()).resolves.toEqual({ counts: {}, rows: [] });
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.auth.rows",
      args: {},
    });
  });

  it("unwraps the notification list", async () => {
    const note = {
      server: "beta",
      state: "needs_reauth",
      title: "beta needs a new login",
      body: "Sign in again.",
      dedupeKey: "beta:needs_reauth",
    };
    invoke.mockResolvedValue({ notifications: [note] });
    await expect(plusAuthNotifications()).resolves.toEqual([note]);
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.auth.notifications",
      args: {},
    });
  });
});

describe("plusAuthFix", () => {
  beforeEach(() => invoke.mockReset());

  it("signs in through plus.auth.login for reauth and reconsent", async () => {
    invoke.mockResolvedValue({ message: "Signed in to figma." });
    for (const action of ["reauth", "reconsent"] as const) {
      await expect(plusAuthFix(fix({ action }))).resolves.toBe("Signed in to figma.");
    }
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith("plus_invoke", {
      command: "plus.auth.login",
      args: { server: "figma" },
    });
  });

  it("runs the route the fix carries", async () => {
    invoke.mockResolvedValue({});
    const retry = fix({
      action: "retry",
      command: null,
      ipc: { command: "plus.auth.probe", args: { server: "figma", force: true } },
    });
    await expect(plusAuthFix(retry)).resolves.toBe("Checked figma again.");
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.auth.probe",
      args: { server: "figma", force: true },
    });
  });

  it("refuses a route outside the auth family and a fix without an action", async () => {
    const foreign = fix({
      ipc: { command: "plus.sync.reset", args: {} },
    });
    await expect(plusAuthFix(foreign)).rejects.toThrow("unsupported fix route");
    await expect(
      plusAuthFix(fix({ action: "fix_config", command: null })),
    ).rejects.toThrow("no one-click fix");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("tells actionable fixes from hints", () => {
    expect(fixIsActionable(fix({ action: "reauth" }))).toBe(true);
    expect(fixIsActionable(fix({ action: "reconsent" }))).toBe(true);
    expect(
      fixIsActionable(
        fix({
          action: "retry",
          command: null,
          ipc: { command: "plus.auth.probe", args: {} },
        }),
      ),
    ).toBe(true);
    expect(fixIsActionable(fix({ action: "fix_config", command: null }))).toBe(false);
  });
});
