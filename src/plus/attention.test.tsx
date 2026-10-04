import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { commandsFixture, commandsFixtureWithMcpCall } from "./fixtures/commandsRegistry";

const { ctlData } = vi.hoisted(() => ({ ctlData: vi.fn() }));
vi.mock("./bridge/ctl", () => ({ ctlData }));

import { forgetAttentionProbe, readAttentionCount, useAttentionCount } from "./attention";

const withAttention = {
  ...commandsFixtureWithMcpCall,
  commands: [
    ...commandsFixture.commands,
    { ...commandsFixture.commands.find((row) => row.id === "attention ls")! },
  ],
};

function serve(replies: { registry?: unknown; attention?: unknown | Error }) {
  ctlData.mockImplementation(async (argv: string[]) => {
    const reply = argv[0] === "commands" ? replies.registry : replies.attention;
    if (reply instanceof Error) throw reply;
    return reply;
  });
}

beforeEach(() => {
  ctlData.mockReset();
  forgetAttentionProbe();
});

describe("readAttentionCount", () => {
  it("is null while the CLI has no attention ls row, and never asks for the list", async () => {
    const without = {
      ...commandsFixture,
      commands: commandsFixture.commands.filter((row) => row.id !== "attention ls"),
    };
    serve({ registry: without });
    await expect(readAttentionCount()).resolves.toBeNull();
    expect(ctlData.mock.calls.map(([argv]) => argv)).toEqual([["commands"]]);
  });

  it("reads what needs the user from attention ls and asks the registry only once", async () => {
    serve({
      registry: withAttention,
      attention: { counts: { needsYou: 4, worthALook: 9 } },
    });
    await expect(readAttentionCount()).resolves.toBe(4);
    serve({
      registry: withAttention,
      attention: { counts: { needsYou: 0, worthALook: 9 } },
    });
    await expect(readAttentionCount()).resolves.toBe(0);
    expect(ctlData.mock.calls.map(([argv]) => argv.join(" "))).toEqual([
      "commands",
      "attention ls",
      "attention ls",
    ]);
  });

  it("asks the registry again after it failed", async () => {
    serve({ registry: new Error("no cli") });
    await expect(readAttentionCount()).rejects.toThrow("no cli");
    serve({
      registry: withAttention,
      attention: { counts: { needsYou: 2, worthALook: 0 } },
    });
    await expect(readAttentionCount()).resolves.toBe(2);
  });
});

describe("useAttentionCount", () => {
  it("polls every minute and keeps the last number when a read fails", async () => {
    vi.useFakeTimers();
    try {
      const read = vi
        .fn<() => Promise<number | null>>()
        .mockResolvedValueOnce(5)
        .mockRejectedValueOnce(new Error("blip"))
        .mockResolvedValueOnce(2);
      const { result } = renderHook(() => useAttentionCount(read));
      expect(result.current).toBeNull();
      await act(async () => {});
      expect(result.current).toBe(5);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(read).toHaveBeenCalledTimes(2);
      expect(result.current).toBe(5);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
      expect(result.current).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops polling when the sidebar goes away", async () => {
    vi.useFakeTimers();
    try {
      const read = vi.fn<() => Promise<number | null>>().mockResolvedValue(1);
      const { unmount } = renderHook(() => useAttentionCount(read));
      await act(async () => {});
      unmount();
      await vi.advanceTimersByTimeAsync(180_000);
      expect(read).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
