import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, info } = vi.hoisted(() => ({ invoke: vi.fn(), info: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("sonner", () => ({ toast: { info } }));

const { UpdateWatcher, UpdateWatchSection } = await import("./UpdateWatch");

const found = {
  server: "g",
  kind: "fork",
  signature: "2",
  title: "g: 2 new commit(s) on origin/main",
  detail: "g is 2 commit(s) behind origin/main.",
  since: 1,
};

const idle = { ran: false, reason: "not-due", raised: [], active: 0, errors: 0 };

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
  info.mockReset();
  visibility("visible");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("UpdateWatcher", () => {
  it("toasts each raised finding once, keyed by its state", async () => {
    invoke.mockResolvedValueOnce({ ...idle, ran: true, reason: null, raised: [found] });
    invoke.mockResolvedValue(idle);
    render(<UpdateWatcher onReview={() => {}} />);
    await settle();
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.update.watchTick",
      args: { force: false },
    });
    expect(info).toHaveBeenCalledTimes(1);
    expect(info).toHaveBeenCalledWith(
      found.title,
      expect.objectContaining({ id: "updates:g:fork:2", description: found.detail }),
    );
    await settle(30 * 60_000);
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(info).toHaveBeenCalledTimes(1);
  });

  it("stays quiet when nothing is due and while the window is hidden", async () => {
    invoke.mockResolvedValue(idle);
    visibility("hidden");
    render(<UpdateWatcher />);
    await settle(30 * 60_000);
    expect(invoke).not.toHaveBeenCalled();
    visibility("visible");
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await settle();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(info).not.toHaveBeenCalled();
  });
});

describe("UpdateWatchSection", () => {
  it("shows the stored settings and writes a change back", async () => {
    invoke.mockResolvedValueOnce({ enabled: true, intervalHours: 24 });
    invoke.mockResolvedValueOnce({ enabled: false, intervalHours: 24 });
    render(<UpdateWatchSection />);
    await settle();
    const box = screen.getByRole("checkbox", { name: "Enabled" });
    expect(box).toBeChecked();
    fireEvent.click(box);
    await settle();
    expect(invoke).toHaveBeenLastCalledWith("plus_invoke", {
      command: "plus.update.watchSettings",
      args: { enabled: false },
    });
    expect(screen.getByRole("checkbox", { name: "Enabled" })).not.toBeChecked();
    expect(screen.getByLabelText("Check interval")).toBeDisabled();
  });
});
