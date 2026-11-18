import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ageLabel,
  registerAttentionFeed,
  type AttentionFeed,
  type AttentionSnapshot,
} from "./attentionFeed";
import { PlusFooterBell } from "./PlusFooterBell";
import { PLUS_NAVIGATE_EVENT } from "./navigate";

function feed(initial: AttentionSnapshot) {
  let snapshot = initial;
  const listeners = new Set<() => void>();
  const value: AttentionFeed = {
    snapshot: () => snapshot,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  return {
    value,
    set(next: AttentionSnapshot) {
      snapshot = next;
      listeners.forEach((listener) => listener());
    },
  };
}

const NOW = Date.now();
const items: AttentionSnapshot = {
  checkedAt: NOW,
  items: [
    {
      id: "a",
      urgency: "needs-you",
      title: "alpha is missing a secret",
      source: "Servers",
      since: NOW - 3_600_000,
    },
    {
      id: "b",
      urgency: "needs-you",
      title: "beta needs a new sign-in",
      source: "Servers",
    },
    { id: "c", urgency: "worth-a-look", title: "Task nightly failed", source: "Tasks" },
  ],
};

afterEach(() => {
  registerAttentionFeed(null);
});

describe("PlusFooterBell", () => {
  it("test01_is_not_drawn_while_no_feed_exists", () => {
    const { container } = render(<PlusFooterBell />);
    expect(container).toBeEmptyDOMElement();
  });

  it("test02_counts_only_the_needs_you_group", () => {
    registerAttentionFeed(feed(items).value);
    render(<PlusFooterBell />);
    expect(
      screen.getByRole("button", { name: "Needs attention, 2" }),
    ).toBeInTheDocument();
  });

  it("test03_lists_both_groups_with_source_and_age", async () => {
    registerAttentionFeed(feed(items).value);
    render(<PlusFooterBell />);
    await userEvent.click(screen.getByRole("button", { name: /Needs attention/ }));
    expect(screen.getByRole("region", { name: "Needs you" })).toHaveTextContent(
      "alpha is missing a secret",
    );
    expect(screen.getByRole("region", { name: "Worth a look" })).toHaveTextContent(
      "Task nightly failed",
    );
    expect(screen.getByText("1 h ago")).toBeInTheDocument();
  });

  it("test04_says_nothing_needs_you_when_the_feed_is_empty", async () => {
    registerAttentionFeed(feed({ items: [], checkedAt: NOW }).value);
    render(<PlusFooterBell />);
    await userEvent.click(
      screen.getByRole("button", { name: "Needs attention, nothing waiting" }),
    );
    expect(screen.getByText(/Nothing needs you/)).toBeInTheDocument();
  });

  it("test05_follows_the_feed_when_it_changes", () => {
    const source = feed({ items: [], checkedAt: NOW });
    registerAttentionFeed(source.value);
    render(<PlusFooterBell />);
    act(() => source.set(items));
    expect(
      screen.getByRole("button", { name: "Needs attention, 2" }),
    ).toBeInTheDocument();
  });

  it("test06_formats_ages", () => {
    expect(ageLabel(NOW - 20_000, NOW)).toBe("just now");
    expect(ageLabel(NOW - 25 * 60_000, NOW)).toBe("25 min ago");
    expect(ageLabel(NOW - 5 * 3_600_000, NOW)).toBe("5 h ago");
    expect(ageLabel(NOW - 3 * 86_400_000, NOW)).toBe("3 d ago");
  });
});

describe("attention.bell rows with an action", () => {
  const withActions: AttentionSnapshot = {
    checkedAt: NOW,
    items: [
      {
        id: "tasks:waiting:r1",
        urgency: "needs-you",
        title: "Task export is waiting for you",
        source: "Tasks",
        action: { label: "Continue", view: "tasks" },
      },
      {
        id: "credentials:secret:alpha",
        urgency: "needs-you",
        title: "alpha is missing a secret",
        source: "Credentials",
        action: { label: "Set secret", view: "servers" },
      },
    ],
  };

  it("test07_the_row_button_opens_the_page_that_owns_the_row_and_closes_the_list", async () => {
    registerAttentionFeed(feed(withActions).value);
    const opened = vi.fn();
    const listener = (event: Event) => opened((event as CustomEvent).detail);
    window.addEventListener(PLUS_NAVIGATE_EVENT, listener);
    try {
      render(<PlusFooterBell />);
      await userEvent.click(screen.getByRole("button", { name: /Needs attention/ }));
      await userEvent.click(screen.getByRole("button", { name: "Set secret" }));
      expect(opened).toHaveBeenCalledWith("servers");
      expect(screen.queryByRole("region", { name: "Needs you" })).not.toBeInTheDocument();
    } finally {
      window.removeEventListener(PLUS_NAVIGATE_EVENT, listener);
    }
  });

  it("test08_a_row_without_an_action_has_no_button", async () => {
    registerAttentionFeed(feed(items).value);
    render(<PlusFooterBell />);
    await userEvent.click(screen.getByRole("button", { name: /Needs attention/ }));
    const list = screen.getByRole("region", { name: "Needs you" });
    expect(within(list).queryByRole("button")).not.toBeInTheDocument();
  });

  it("test09_an_area_that_could_not_be_read_is_named_instead_of_an_all_clear", async () => {
    registerAttentionFeed(
      feed({ items: [], checkedAt: NOW, unchecked: ["tasks"] }).value,
    );
    render(<PlusFooterBell />);
    await userEvent.click(
      screen.getByRole("button", { name: "Needs attention, not fully checked" }),
    );
    expect(screen.getByText(/Not checked: tasks/)).toBeInTheDocument();
    expect(screen.queryByText(/Nothing needs you/)).not.toBeInTheDocument();
  });
});
