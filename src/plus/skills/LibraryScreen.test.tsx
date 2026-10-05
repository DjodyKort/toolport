import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./SkillsTab", () => ({ SkillsTab: () => <p>Skills panel</p> }));
vi.mock("../plugins/PluginsTab", () => ({ PluginsTab: () => <p>Plugins panel</p> }));
vi.mock("./sources/SourcesTab", () => ({ SourcesTab: () => <p>Sources panel</p> }));
vi.mock("../agents", () => ({
  AgentsTab: () => <p>Agents panel</p>,
  StylesTab: () => <p>Styles panel</p>,
}));

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

  it("mounts the Agents and Styles panels as tabs", async () => {
    const user = userEvent.setup();
    render(<LibraryScreen onOpenCommands={vi.fn()} />);
    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    expect(screen.getByText("Agents panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
    await user.click(screen.getByRole("tab", { name: "Styles" }));
    expect(screen.getByText("Styles panel")).toBeInTheDocument();
  });

  it("mounts the Sources panel lazily as a tab", async () => {
    const user = userEvent.setup();
    render(<LibraryScreen onOpenCommands={vi.fn()} />);
    expect(screen.queryByText("Sources panel")).toBeNull();
    await user.click(await screen.findByRole("tab", { name: "Sources" }));
    expect(await screen.findByText("Sources panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
  });

  it("mounts the Plugins panel lazily when its tab is opened", async () => {
    const user = userEvent.setup();
    render(<LibraryScreen onOpenCommands={vi.fn()} />);
    await user.click(await screen.findByRole("tab", { name: "Plugins" }));
    expect(await screen.findByText("Plugins panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
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
