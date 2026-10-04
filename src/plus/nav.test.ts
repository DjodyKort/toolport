import { describe, expect, it } from "vitest";
import { SHORTCUT_VIEWS } from "@/lib/shortcuts";
import {
  NAV_GROUPS,
  PLUS_SCREENS,
  PLUS_VIEWS,
  isPlusView,
  navItemActive,
  type NavItem,
} from "./nav";
import { NOT_BUILT_TABS } from "./notBuiltTabs";

const items = NAV_GROUPS.flatMap((group) => group.items);

describe("sidebar B", () => {
  it("has the groups and the 16 labels of the approved mockup, in order", () => {
    expect(NAV_GROUPS.map((group) => group.label)).toEqual([
      null,
      "Servers",
      "Claude",
      "Agents",
      "More",
    ]);
    expect(items.map((item) => item.label)).toEqual([
      "Attention",
      "Servers",
      "Clients",
      "Browse catalog",
      "Playground",
      "Library",
      "Context",
      "Tokens",
      "Tasks",
      "Agent rules",
      "Agent activity",
      "Agent permissions",
      "Activity",
      "Teams",
      "System",
      "Settings",
    ]);
  });

  it("gives every entry its own view, and a screen for every Toolport+ view", () => {
    expect(new Set(items.map((item) => item.view)).size).toBe(16);
    for (const view of PLUS_VIEWS) {
      expect(isPlusView(view)).toBe(true);
      const screen = PLUS_SCREENS[view];
      expect(screen.title).toBeTruthy();
      expect(screen.builtBy).toMatch(/^MIG-[A-Z]+-\d+$/);
      const tabs = NOT_BUILT_TABS[view] ?? [];
      for (const tab of tabs) expect(tab.builtBy).toMatch(/^MIG-[A-Z]+-\d+$/);
      const ids = tabs.map((tab) => tab.id);
      expect(new Set(ids).size).toBe(ids.length);
    }
    expect(isPlusView("servers")).toBe(false);
    expect(isPlusView("settings")).toBe(false);
  });

  it("keeps every view the number keys open reachable from the sidebar", () => {
    const reachable = new Set(items.map((item) => item.view));
    for (const view of SHORTCUT_VIEWS) expect(reachable.has(view)).toBe(true);
  });

  it("puts the counters on Attention and Settings only", () => {
    expect(
      items.filter((item) => item.badge).map((item) => [item.label, item.badge]),
    ).toEqual([
      ["Attention", "attention"],
      ["Settings", "quarantine"],
    ]);
  });

  it("highlights Settings while the All commands page, which lives under it, is open", () => {
    const settings = items.find((item) => item.view === "settings") as NavItem;
    expect(navItemActive(settings, "settings")).toBe(true);
    expect(navItemActive(settings, "commands")).toBe(true);
    expect(navItemActive(settings, "system")).toBe(false);
    const system = items.find((item) => item.view === "system") as NavItem;
    expect(navItemActive(system, "commands")).toBe(false);
  });

  it("offers a command group for each tab that has a command", () => {
    const groups = Object.values(NOT_BUILT_TABS)
      .flatMap((tabs) => tabs ?? [])
      .flatMap((tab) => (tab.group ? [tab.group] : []));
    expect(groups.length).toBeGreaterThan(5);
  });
});
