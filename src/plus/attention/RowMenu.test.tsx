import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Toaster } from "@/components/ui/sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { registerAttentionFeed } from "../shell/attentionFeed";
import { PlusFooterBell } from "../shell/PlusFooterBellSlot";
import { PLUS_NAVIGATE_EVENT } from "../shell/navigate";
import { localTimestamp, morning, snoozeChoices } from "./attentionCtl";
import { choicesFor } from "./hideActions";

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

async function openBell(name = "Needs attention, 3") {
  render(
    <>
      <PlusFooterBell />
      <Toaster />
    </>,
  );
  await userEvent.click(await screen.findByRole("button", { name }));
}

async function openMenu(title: string) {
  await userEvent.click(screen.getByRole("button", { name: `Actions for ${title}` }));
  return screen.findByRole("menu");
}

describe("attention-manage.hide: the actions of a bell row", () => {
  it("test01_a_needs_you_row_offers_only_snoozes_of_at_most_a_week", () => {
    const labels = choicesFor({
      id: "a",
      urgency: "needs-you",
      title: "a",
      source: "Tasks",
    }).map((choice) => choice.label);
    expect(labels).toEqual([
      "Snooze 1 hour",
      "Snooze until tomorrow 08:00",
      "Snooze for a week",
    ]);
  });

  it("test02_a_skippable_row_offers_skip_and_one_without_a_version_offers_on_purpose", () => {
    const base = {
      id: "a",
      urgency: "worth-a-look" as const,
      title: "a",
      source: "Updates",
    };
    expect(choicesFor({ ...base, skip: { label: "1.5.0" } })[0]).toMatchObject({
      kind: "skip",
      label: "Skip 1.5.0",
    });
    expect(choicesFor(base)[0]).toMatchObject({ kind: "dismiss" });
  });

  it("test03_a_snooze_until_the_morning_names_the_local_offset_not_utc", () => {
    const now = new Date(2026, 10, 18, 14, 30);
    const until = snoozeChoices(now, false)[1].args[1];
    expect(until).toMatch(/^2026-11-19T08:00:00[+-]\d{2}:\d{2}$/);
    expect(new Date(until).getTime()).toBe(morning(now, 1).getTime());
    expect(localTimestamp(morning(now, 1))).toBe(until);
  });

  it("test04_the_next_monday_is_a_later_monday_even_on_a_monday", () => {
    const monday = new Date(2026, 10, 16, 9, 0);
    const next = morning(monday, 0, true);
    expect(next.getDay()).toBe(1);
    expect(next.getDate()).toBe(23);
  });

  it("test05_snoozing_hides_the_row_and_the_toast_undo_brings_it_back", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    await openBell();
    await openMenu("alpha-api is missing a secret");
    await userEvent.click(screen.getByRole("menuitem", { name: "Snooze 1 hour" }));
    expect(
      await screen.findByText(/Hidden until .*: alpha-api is missing a secret/),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        screen.queryByText("alpha-api is missing a secret", { selector: "p" }),
      ).toBeNull(),
    );
    expect(screen.getByText(/1 hidden/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    const bell = await screen.findByRole("button", { name: "Needs attention, 3" });
    if (!screen.queryByText("alpha-api is missing a secret")) await userEvent.click(bell);
    expect(await screen.findByText("alpha-api is missing a secret")).toBeInTheDocument();
  });

  it("test06_skipping_an_update_says_it_returns_with_the_next_version", async () => {
    resetPlusFixture({ attention: { scenario: "updates" } });
    await openBell("Needs attention, 3");
    await openMenu("The skill tap acme-skills has 3 new commits");
    await userEvent.click(screen.getByRole("menuitem", { name: "Skip 5e6f7a8b" }));
    expect(
      await screen.findByText(/Skipped 5e6f7a8b\. It returns when this changes/),
    ).toBeVisible();
  });

  it("test07_a_failed_write_is_reported_and_the_row_stays", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    await openBell();
    const menu = await openMenu("Task nightly-sync failed");
    resetPlusFixture({ attention: { scenario: "clear" } });
    await userEvent.click(
      within(menu).getByRole("menuitem", { name: "This is on purpose" }),
    );
    expect(await screen.findByText("Could not hide the row")).toBeVisible();
    expect(screen.getByText("Task nightly-sync failed")).toBeInTheDocument();
  });

  it("test08_open_goes_to_the_page_that_owns_the_row_and_the_menu_is_keyboard_reachable", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    const opened = vi.fn();
    const listener = (event: Event) => opened((event as CustomEvent).detail);
    window.addEventListener(PLUS_NAVIGATE_EVENT, listener);
    try {
      await openBell();
      const trigger = screen.getByRole("button", {
        name: "Actions for alpha-api is missing a secret",
      });
      trigger.focus();
      await userEvent.keyboard("{Enter}");
      await userEvent.click(
        await screen.findByRole("menuitem", { name: "Open in Servers" }),
      );
      expect(opened).toHaveBeenCalledWith("servers");
    } finally {
      window.removeEventListener(PLUS_NAVIGATE_EVENT, listener);
    }
  });

  it("test09_control_z_while_the_toast_shows_undoes_the_change", async () => {
    resetPlusFixture({ attention: { scenario: "busy" } });
    await openBell();
    await openMenu("alpha-api is missing a secret");
    await userEvent.click(screen.getByRole("menuitem", { name: "Snooze for a week" }));
    await screen.findByText(/Hidden until/);
    await userEvent.keyboard("{Control>}z{/Control}");
    expect(await screen.findByText("alpha-api is missing a secret")).toBeInTheDocument();
  });
});
