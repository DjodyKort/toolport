import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./CompressionTab", () => ({ CompressionTab: () => <p>Compression panel</p> }));
vi.mock("../usage/UsageTab", () => ({ UsageTab: () => <p>Usage panel</p> }));

import { PANELS, TokensScreen } from "./TokensScreen";

describe("Tokens screen", () => {
  it("has the Usage and Compression tabs and opens on the Usage panel", async () => {
    render(<TokensScreen onOpenCommands={vi.fn()} />);
    const tabs = await screen.findByRole("tablist", { name: "Tokens sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Usage", "Compression"]);
    expect(screen.getByText("Usage panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
    expect(screen.queryByText("Compression panel")).toBeNull();
  });

  it("shows the Compression panel on its tab and starts there when asked", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<TokensScreen onOpenCommands={vi.fn()} />);
    await user.click(await screen.findByRole("tab", { name: "Compression" }));
    expect(screen.getByText("Compression panel")).toBeInTheDocument();
    expect(screen.queryByText("Usage panel")).toBeNull();
    expect(screen.queryByText("Not built yet")).toBeNull();
    unmount();
    render(<TokensScreen initialTab="compression" onOpenCommands={vi.fn()} />);
    expect(await screen.findByText("Compression panel")).toBeInTheDocument();
  });

  it("keeps the marked placeholder for a tab that has no panel", async () => {
    const user = userEvent.setup();
    const open = vi.fn();
    const usage = PANELS.usage;
    delete PANELS.usage;
    try {
      render(<TokensScreen onOpenCommands={open} />);
      expect(await screen.findByText("Not built yet")).toBeInTheDocument();
      expect(screen.getByText(/built by MIG-GUI-7\b/)).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Open All commands" }));
      expect(open).toHaveBeenCalledWith("usage");
    } finally {
      PANELS.usage = usage;
    }
  });
});
