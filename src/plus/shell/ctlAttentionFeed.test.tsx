import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { PlusFooterBell } from "./PlusFooterBellSlot";
import { registerAttentionFeed, useAttentionSnapshot } from "./attentionFeed";
import { startAttentionFeed, toSnapshot } from "./ctlAttentionFeed";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

function Probe() {
  const snapshot = useAttentionSnapshot();
  return <p>{snapshot ? `${snapshot.items.length} rows` : "no feed"}</p>;
}

beforeEach(() => registerAttentionFeed(null));
afterEach(() => registerAttentionFeed(null));

describe("the attention feed read from toolportctl", () => {
  it("test01_a_failed_first_answer_keeps_the_bell_out", async () => {
    resetPlusFixture({});
    const stop = startAttentionFeed();
    render(<Probe />);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.getByText("no feed")).toBeInTheDocument();
    stop();
  });

  it("test02_the_first_answer_registers_the_feed_with_rows_in_order", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    const stop = startAttentionFeed();
    render(<Probe />);
    expect(await screen.findByText("5 rows")).toBeInTheDocument();
    stop();
    await waitFor(() => expect(screen.getByText("no feed")).toBeInTheDocument());
  });

  it("test03_the_slot_starts_the_feed_and_the_bell_opens_a_row_that_navigates", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    render(<PlusFooterBell />);
    const bell = await screen.findByRole("button", { name: "Needs attention, 3" });
    await userEvent.click(bell);
    expect(screen.getByText("alpha-api is missing a secret")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set secret" })).toBeInTheDocument();
  });

  it("test04_toSnapshot_maps_screens_to_views_and_names_unread_areas", () => {
    const snapshot = toSnapshot({
      checkedAt: "2026-11-17T08:58:00Z",
      counts: { needsYou: 1, worthALook: 0 },
      checks: [
        { name: "credentials", ok: true },
        { name: "library", ok: false, error: "unreadable" },
      ],
      items: [
        {
          id: "library:held",
          urgency: "needs-you",
          title: "2 changes wait for you in the library",
          detail: "",
          source: "Library",
          action: { label: "Review", screen: "library" },
        },
      ],
    });
    expect(snapshot.unchecked).toEqual(["library"]);
    expect(snapshot.checkedAt).toBe(Date.parse("2026-11-17T08:58:00Z"));
    expect(snapshot.items[0]).toMatchObject({
      detail: undefined,
      since: undefined,
      action: { label: "Review", view: "library" },
    });
  });

  it("test05_a_later_answer_replaces_the_rows_without_a_second_registration", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const stop = startAttentionFeed(1000);
      render(<Probe />);
      expect(await screen.findByText("5 rows")).toBeInTheDocument();
      resetPlusFixture({ attention: { scenario: "clear" } });
      await vi.advanceTimersByTimeAsync(1500);
      expect(await screen.findByText("0 rows")).toBeInTheDocument();
      stop();
    } finally {
      vi.useRealTimers();
    }
  });
});
