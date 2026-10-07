import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { CatalogIndexWatcher } = await import("./CatalogIndexWatch");

const idle = {
  count: 1123,
  lastSyncedAt: "2026-10-07T09:00:00Z",
  complete: true,
  running: false,
  ran: false,
  reason: "not-due",
};

function visibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
}

async function settle(ms = 0) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  invoke.mockReset();
  invoke.mockResolvedValue(idle);
  visibility("visible");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("CatalogIndexWatcher", () => {
  it("asks for a scheduled sync at app start and every half hour, and the backend decides", async () => {
    render(<CatalogIndexWatcher />);
    await settle();
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.catalog.indexTick",
      args: { force: false },
    });
    expect(invoke).toHaveBeenCalledTimes(1);

    await settle(30 * 60_000);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("keeps the index fresh with a hidden window and asks again when it shows", async () => {
    visibility("hidden");
    render(<CatalogIndexWatcher />);
    await settle(30 * 60_000);
    expect(invoke).toHaveBeenCalledTimes(2);

    visibility("visible");
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await settle();
    expect(invoke).toHaveBeenCalledTimes(3);
  });

  it("ignores a backend that cannot answer", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    render(<CatalogIndexWatcher />);
    await settle(30 * 60_000);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("stops polling when it unmounts", async () => {
    const { unmount } = render(<CatalogIndexWatcher />);
    await settle();
    unmount();
    await settle(2 * 30 * 60_000);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
