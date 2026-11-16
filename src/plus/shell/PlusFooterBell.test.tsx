import { afterEach, describe, expect, it } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ageLabel,
  registerAttentionFeed,
  type AttentionFeed,
  type AttentionSnapshot,
} from "./attentionFeed";
import { PlusFooterBell } from "./PlusFooterBell";

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
