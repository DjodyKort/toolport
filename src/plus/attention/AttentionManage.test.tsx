import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Toaster } from "@/components/ui/sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import AttentionManage from "./AttentionManage";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

function mount() {
  render(
    <>
      <AttentionManage />
      <Toaster />
    </>,
  );
}

beforeEach(() => resetPlusFixture({ attention: { scenario: "updates" } }));

describe("Settings > Toolport+ > Attention", () => {
  it("test01_lists_every_open_row_with_its_id_and_filters_by_level", async () => {
    mount();
    const open = await screen.findByRole("region", { name: "Open now" });
    expect(within(open).getAllByRole("listitem")).toHaveLength(6);
    expect(within(open).getByText("credentials:secret:alpha-api")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Needs you 3" }));
    expect(within(open).getAllByRole("listitem")).toHaveLength(3);
    await userEvent.click(screen.getByRole("button", { name: "Worth a look 3" }));
    expect(within(open).queryByText("alpha-api is missing a secret")).toBeNull();
  });

  it("test02_a_hidden_row_says_why_and_restore_brings_it_back", async () => {
    mount();
    const open = await screen.findByRole("region", { name: "Open now" });
    await userEvent.click(
      within(open).getByRole("button", {
        name: "Actions for The skill tap acme-skills has 3 new commits",
      }),
    );
    await userEvent.click(await screen.findByRole("menuitem", { name: "Skip 5e6f7a8b" }));
    const hidden = await screen.findByRole("region", { name: "Hidden" });
    expect(
      await within(hidden).findByText(/Skipped: this version only/),
    ).toBeInTheDocument();
    await userEvent.click(within(hidden).getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(within(hidden).queryByText(/Skipped:/)).toBeNull());
    expect(
      within(open).getByText("The skill tap acme-skills has 3 new commits"),
    ).toBeInTheDocument();
  });

  it("test03_attention-manage.settings_master_switch_enables_the_categories_and_the_choice_is_kept", async () => {
    mount();
    const master = await screen.findByRole("switch", {
      name: "Notify me when something needs me",
    });
    const credentials = screen.getByRole("switch", {
      name: "Notify me about Credentials",
    });
    expect(master).not.toBeChecked();
    expect(credentials).toBeDisabled();
    await userEvent.click(master);
    await waitFor(() => expect(credentials).toBeEnabled());
    expect(credentials).toBeChecked();
    expect(
      screen.getByRole("switch", { name: "Notify me about Updates" }),
    ).not.toBeChecked();
    await userEvent.click(
      screen.getByRole("switch", { name: "Notify me about Updates" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "Notify me about Updates" }),
      ).toBeChecked(),
    );
  });

  it("test04_switching_a_check_off_marks_it_off_and_disables_its_notify_switch", async () => {
    mount();
    await userEvent.click(
      await screen.findByRole("switch", { name: "Notify me when something needs me" }),
    );
    await userEvent.click(await screen.findByRole("switch", { name: "Check Tasks" }));
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "Notify me about Tasks" }),
      ).toBeDisabled(),
    );
  });

  it("test05_attention-manage.check_reports_what_it_looked_at", async () => {
    mount();
    await userEvent.click(
      await screen.findByRole("button", { name: /Check for updates now/ }),
    );
    expect(await screen.findByText("Looked at 1 source")).toBeVisible();
  });

  it("test06_the_test_notification_needs_the_master_switch_and_reports_success", async () => {
    mount();
    const send = await screen.findByRole("button", { name: "Send a test" });
    expect(send).toBeDisabled();
    await userEvent.click(
      screen.getByRole("switch", { name: "Notify me when something needs me" }),
    );
    await waitFor(() => expect(send).toBeEnabled());
    await userEvent.click(send);
    expect(
      await screen.findByText("The test notification was handed to the system"),
    ).toBeVisible();
    expect(screen.getByText(/does not tell Toolport\+ whether/)).toBeVisible();
  });

  it("test07_a_list_that_cannot_be_read_says_so_with_a_retry", async () => {
    resetPlusFixture({});
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The attention list could not be read",
    );
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });

  it("test08_the_access_check_has_its_own_switches_and_lists_its_rows", async () => {
    resetPlusFixture({ attention: { scenario: "access" } });
    mount();
    const open = await screen.findByRole("region", { name: "Open now" });
    expect(within(open).getByText("access:new:epsilon-db")).toBeInTheDocument();
    expect(within(open).getByText("access:unreached:zeta-cache")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Check Access" })).toBeChecked();
    await userEvent.click(
      await screen.findByRole("switch", { name: "Notify me when something needs me" }),
    );
    expect(
      await screen.findByRole("switch", { name: "Notify me about Access" }),
    ).not.toBeChecked();
  });

  it("test09_the_access_check_sets_how_many_days_make_a_server_stale", async () => {
    mount();
    const days = await screen.findByRole("spinbutton", {
      name: "Days until a server counts as stale",
    });
    expect(days).toHaveValue(30);
    await userEvent.clear(days);
    await userEvent.type(days, "14");
    await userEvent.tab();
    await waitFor(() => expect(days).toHaveValue(14));
    await userEvent.clear(days);
    await userEvent.type(days, "999");
    await userEvent.tab();
    await waitFor(() => expect(days).toHaveValue(14));
  });

  it("test10_a_platform_that_reports_allowed_confirms_the_test_and_shows_the_allowed_state", async () => {
    resetPlusFixture({ attention: { scenario: "busy", permission: "allowed" } });
    mount();
    expect(
      await screen.findByText("macOS allows notifications from Toolport+"),
    ).toBeVisible();
    await userEvent.click(
      screen.getByRole("switch", { name: "Notify me when something needs me" }),
    );
    const send = screen.getByRole("button", { name: "Send a test" });
    await waitFor(() => expect(send).toBeEnabled());
    await userEvent.click(send);
    expect(await screen.findByText("A test notification was sent")).toBeVisible();
  });

  it("test11_blocked_shows_how_to_fix_it_and_the_test_fails_instead_of_saying_sent", async () => {
    resetPlusFixture({ attention: { scenario: "busy", permission: "blocked" } });
    mount();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("macOS blocks notifications from Toolport+");
    expect(alert).toHaveTextContent("System Settings > Notifications > Toolport+");
    await userEvent.click(
      screen.getByRole("switch", { name: "Notify me when something needs me" }),
    );
    const send = screen.getByRole("button", { name: "Send a test" });
    await waitFor(() => expect(send).toBeEnabled());
    await userEvent.click(send);
    expect(
      await screen.findByText("macOS blocks notifications from Toolport+", {
        selector: "[data-title]",
      }),
    ).toBeVisible();
    expect(screen.queryByText("A test notification was sent")).toBeNull();
  });

  it("test12_open_system_settings_asks_the_app_to_open_them", async () => {
    resetPlusFixture({ attention: { scenario: "busy", permission: "blocked" } });
    mount();
    await userEvent.click(
      await screen.findByRole("button", { name: "Open System Settings" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Open System Settings" })).toBeEnabled(),
    );
    expect(screen.queryByText("The system settings did not open")).toBeNull();
  });

  it("test13_not_asked_says_the_first_test_asks_once", async () => {
    resetPlusFixture({ attention: { scenario: "busy", permission: "not-asked" } });
    mount();
    expect(await screen.findByText("macOS has not been asked yet")).toBeVisible();
  });

  it("test14_an_unreadable_permission_is_named_as_unknown_with_the_way_to_check", async () => {
    mount();
    expect(
      await screen.findByText("Toolport+ cannot see whether macOS allows notifications"),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Open System Settings" }),
    ).toBeInTheDocument();
  });
});
