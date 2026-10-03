import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { plusInvoke, plusPing } = await import("./api");

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

  it("pings with empty args", async () => {
    invoke.mockResolvedValue({
      name: "toolport-plus",
      version: "1",
      forkEgressDisabled: true,
    });
    const ping = await plusPing();
    expect(ping.name).toBe("toolport-plus");
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.ping",
      args: {},
    });
  });
});
