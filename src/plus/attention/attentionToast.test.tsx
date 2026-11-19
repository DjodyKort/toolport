import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Toaster } from "@/components/ui/sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { registerAttentionFeed } from "../shell/attentionFeed";
import { PLUS_NAVIGATE_EVENT } from "../shell/navigate";
import { AttentionBanner } from "./AttentionBanner";
import {
  ATTENTION_TOAST_EVENT,
  showAttentionToast,
  startAttentionToasts,
  type AttentionToast,
} from "./attentionToast";

const listeners = new Map<string, (event: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return () => listeners.delete(name);
  }),
}));

const NEWS: AttentionToast = {
  title: "beta-mail needs a new sign-in",
  body: "Its 12 tools fail until you sign in. Open Toolport+ to fix it.",
  rows: [{ id: "credentials:login:beta-mail", screen: "servers" }],
};

afterEach(() => {
  registerAttentionFeed(null);
  listeners.clear();
});

describe("the in-app toast of the notifier", () => {
  it("test01_a_focused_window_on_another_page_shows_the_news_with_an_open_button", async () => {
    render(<Toaster />);
    showAttentionToast(NEWS);
    expect(await screen.findByText("beta-mail needs a new sign-in")).toBeVisible();
    const opened = vi.fn();
    window.addEventListener(PLUS_NAVIGATE_EVENT, (event) =>
      opened((event as CustomEvent).detail),
    );
    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(opened).toHaveBeenCalledWith("servers");
  });

  it("test02_no_toast_while_the_page_that_shows_the_row_is_in_front", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    render(
      <>
        <AttentionBanner page="servers" />
        <Toaster />
      </>,
    );
    showAttentionToast(NEWS);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(
      screen.queryByText("Its 12 tools fail until you sign in.", { exact: false }),
    ).toBeNull();
  });

  it("test03_one_row_on_a_page_that_is_not_shown_is_enough_for_a_toast", async () => {
    render(
      <>
        <AttentionBanner page="servers" />
        <Toaster />
      </>,
    );
    showAttentionToast({
      ...NEWS,
      title: "2 things need you",
      rows: [...NEWS.rows, { id: "tasks:waiting:run-x", screen: "tasks" }],
    });
    expect(await screen.findByText("2 things need you")).toBeVisible();
  });

  it("test04_the_event_from_the_app_shows_the_toast_and_stopping_removes_the_listener", async () => {
    render(<Toaster />);
    const stop = startAttentionToasts();
    await vi.waitFor(() => expect(listeners.has(ATTENTION_TOAST_EVENT)).toBe(true));
    listeners.get(ATTENTION_TOAST_EVENT)?.({ payload: NEWS });
    expect(await screen.findByText("beta-mail needs a new sign-in")).toBeVisible();
    stop();
    await vi.waitFor(() => expect(listeners.has(ATTENTION_TOAST_EVENT)).toBe(false));
  });
});
