import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./CompressionTab", () => ({ CompressionTab: () => <p>Compression panel</p> }));

import { TokensScreen } from "./TokensScreen";

describe("Tokens screen", () => {
  it("has the Usage and Compression tabs and opens on Usage as a marked placeholder", async () => {
    render(<TokensScreen onOpenCommands={vi.fn()} />);
    const tabs = await screen.findByRole("tablist", { name: "Tokens sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Usage", "Compression"]);
    expect(screen.getByText("Not built yet")).toBeInTheDocument();
    expect(screen.getByText(/built by MIG-GUI-7\b/)).toBeInTheDocument();
    expect(screen.queryByText("Compression panel")).toBeNull();
  });

  it("shows the Compression panel on its tab and starts there when asked", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<TokensScreen onOpenCommands={vi.fn()} />);
    await user.click(await screen.findByRole("tab", { name: "Compression" }));
    expect(screen.getByText("Compression panel")).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
    unmount();
    render(<TokensScreen initialTab="compression" onOpenCommands={vi.fn()} />);
    expect(await screen.findByText("Compression panel")).toBeInTheDocument();
  });

  it("opens All commands on the usage group from the placeholder", async () => {
    const user = userEvent.setup();
    const open = vi.fn();
    render(<TokensScreen onOpenCommands={open} />);
    await user.click(await screen.findByRole("button", { name: "Open All commands" }));
    expect(open).toHaveBeenCalledWith("usage");
  });
});
