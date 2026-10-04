import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./SkillsTab", () => ({ SkillsTab: () => <p>Skills panel</p> }));

import { LibraryScreen } from "./LibraryScreen";

describe("Library screen", () => {
  it("has the five tabs of the mockup and opens on Skills", async () => {
    render(<LibraryScreen onOpenCommands={vi.fn()} />);
    const tabs = await screen.findByRole("tablist", { name: "Library sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Skills", "Agents", "Styles", "Plugins", "Sources"]);
    expect(within(tabs).getByRole("tab", { name: "Skills" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Skills panel")).toBeInTheDocument();
  });

  it("marks the tabs no item has built yet and names the item that builds each", async () => {
    const user = userEvent.setup();
    render(<LibraryScreen onOpenCommands={vi.fn()} />);
    const tabs = await screen.findByRole("tablist", { name: "Library sections" });
    for (const [tab, item] of [
      ["Agents", "MIG-GUI-4"],
      ["Styles", "MIG-GUI-4"],
      ["Plugins", "MIG-GUI-12"],
      ["Sources", "MIG-GUI-10"],
    ]) {
      await user.click(within(tabs).getByRole("tab", { name: tab }));
      expect(screen.getByText("Not built yet")).toBeInTheDocument();
      expect(screen.getByText(new RegExp(`built by ${item}\\b`))).toBeInTheDocument();
      expect(screen.queryByText("Skills panel")).toBeNull();
    }
  });

  it("opens the All commands page on the group of the tab it was left from", async () => {
    const user = userEvent.setup();
    const open = vi.fn();
    render(<LibraryScreen onOpenCommands={open} />);
    await user.click(await screen.findByRole("tab", { name: "Styles" }));
    await user.click(screen.getByRole("button", { name: "Open All commands" }));
    expect(open).toHaveBeenCalledWith("styles");
  });

  it("starts on the tab it is asked to and moves between tabs with the arrow keys", async () => {
    const user = userEvent.setup();
    render(<LibraryScreen initialTab="plugins" onOpenCommands={vi.fn()} />);
    const plugins = await screen.findByRole("tab", { name: "Plugins" });
    expect(plugins).toHaveAttribute("aria-selected", "true");
    plugins.focus();
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "Styles" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});
