import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Toaster } from "@/components/ui/sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { registerAttentionFeed } from "../shell/attentionFeed";
import { PlusFooterBell } from "../shell/PlusFooterBellSlot";
import { AttentionBanner } from "./AttentionBanner";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

beforeEach(() => registerAttentionFeed(null));
afterEach(() => registerAttentionFeed(null));

function mount(page: "servers" | "tasks" | "library", except?: string[]) {
  render(
    <>
      <PlusFooterBell />
      <AttentionBanner page={page} except={except} />
      <Toaster />
    </>,
  );
}

describe("banners on a page", () => {
  it("test01_the_servers_page_shows_its_needs_you_rows_with_the_source_label", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    mount("servers");
    expect(await screen.findByText("beta-mail needs a new sign-in")).toBeInTheDocument();
    expect(screen.getByText("alpha-api is missing a secret")).toBeInTheDocument();
    expect(screen.queryByText("Task export-notes is waiting for you")).toBeNull();
    expect(screen.getAllByText("Attention · Credentials")).toHaveLength(2);
  });

  it("test02_rows_of_other_pages_and_rows_the_page_draws_itself_are_left_out", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    mount("tasks", ["tasks:waiting:"]);
    expect(await screen.findByText("Task nightly-sync failed")).toBeInTheDocument();
    expect(screen.queryByText("Task export-notes is waiting for you")).toBeNull();
    expect(screen.queryByText("alpha-api is missing a secret")).toBeNull();
  });

  it("test03_worth_a_look_rows_collapse_into_one_line", async () => {
    resetPlusFixture({ attention: { scenario: "updates" } });
    mount("library");
    expect(await screen.findByText("2 things are worth a look")).toBeInTheDocument();
  });

  it("test04_snoozing_in_the_banner_hides_the_row_in_the_bell_too", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    mount("servers");
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Actions for alpha-api is missing a secret",
      }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Snooze for a week" }),
    );
    await waitFor(() =>
      expect(screen.queryByText("alpha-api is missing a secret")).toBeNull(),
    );
    expect(
      await screen.findByRole("button", { name: "Needs attention, 2" }),
    ).toBeInTheDocument();
  });

  it("test05_nothing_is_drawn_while_there_is_no_feed", () => {
    const { container } = render(<AttentionBanner page="servers" />);
    expect(container).toBeEmptyDOMElement();
  });
});
