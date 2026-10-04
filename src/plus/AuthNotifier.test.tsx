import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, warning } = vi.hoisted(() => ({ invoke: vi.fn(), warning: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("sonner", () => ({ toast: { warning } }));

const { AuthNotifier } = await import("./AuthNotifier");

const note = {
  server: "beta",
  state: "needs_reauth",
  title: "beta needs a new login",
  body: "beta can no longer authenticate (invalid_grant). Sign in again.",
  dedupeKey: "beta:needs_reauth",
};

function visibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
}

async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  invoke.mockReset();
  warning.mockReset();
  visibility("visible");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("AuthNotifier", () => {
  it("raises one toast per notification, keyed so a repeat replaces it", async () => {
    invoke.mockResolvedValue({ notifications: [note] });
    render(<AuthNotifier />);
    await settle();
    expect(invoke).toHaveBeenCalledWith("plus_invoke", {
      command: "plus.auth.notifications",
      args: {},
    });
    expect(warning).toHaveBeenCalledTimes(1);
    expect(warning).toHaveBeenCalledWith(
      "beta needs a new login",
      expect.objectContaining({
        id: "beta:needs_reauth",
        description: note.body,
        action: undefined,
      }),
    );
  });

  it("offers a Review action that calls back", async () => {
    invoke.mockResolvedValue({ notifications: [note] });
    const onReview = vi.fn();
    render(<AuthNotifier onReview={onReview} />);
    await settle();
    const options = warning.mock.calls[0][1] as {
      action: { label: string; onClick: () => void };
    };
    expect(options.action.label).toBe("Review");
    options.action.onClick();
    expect(onReview).toHaveBeenCalledTimes(1);
  });

  it("polls every minute and shows nothing when there is nothing new", async () => {
    invoke.mockResolvedValue({ notifications: [] });
    render(<AuthNotifier />);
    await settle();
    expect(invoke).toHaveBeenCalledTimes(1);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(warning).not.toHaveBeenCalled();
  });

  it("does not spend an edge while the window is hidden, and catches up when shown", async () => {
    visibility("hidden");
    invoke.mockResolvedValue({ notifications: [note] });
    render(<AuthNotifier />);
    await settle();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(invoke).not.toHaveBeenCalled();
    visibility("visible");
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(warning).toHaveBeenCalledTimes(1);
  });

  it("ignores a backend that cannot answer and stops polling on unmount", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    const { unmount } = render(<AuthNotifier />);
    await settle();
    expect(warning).not.toHaveBeenCalled();
    unmount();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(180_000);
    });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
