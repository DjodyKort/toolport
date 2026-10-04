import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "../PlusViews";
import { isPlusView, NAV_GROUPS, navItemActive, PLUS_SCREENS } from "../nav";
import { AgentsTab, StylesTab } from "./index";
import { createBridge, wire } from "./testkit";

beforeEach(() => {
  wire({ invoke, listen }, createBridge());
});

describe("the transitional agents view", () => {
  it("is a Plus view with its own title, kept under Library in the sidebar", () => {
    expect(isPlusView("agents")).toBe(true);
    expect(PLUS_SCREENS.agents.title).toBe("Agents & styles");
    const library = NAV_GROUPS.flatMap((g) => g.items).find(
      (i) => i.label === "Library",
    )!;
    expect(navItemActive(library, "agents")).toBe(true);
  });

  it("opens on the Agents tab and switches to Styles", async () => {
    const user = userEvent.setup();
    render(<PlusViews view="agents" onSelectView={vi.fn()} />);
    expect(
      await screen.findByRole("tab", { name: "Agents", selected: true }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("list", { name: /Output of scout/ }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Styles" }));
    expect(await screen.findByText("No output styles yet")).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Agents" }));
    expect(
      await screen.findByRole("list", { name: /Output of scout/ }),
    ).toBeInTheDocument();
  });

  it("is reached from the Agents and Styles tabs of the Library placeholder", async () => {
    const user = userEvent.setup();
    const onSelectView = vi.fn();
    render(<PlusViews view="library" onSelectView={onSelectView} />);
    await user.click(await screen.findByRole("tab", { name: "Agents" }));
    await user.click(await screen.findByRole("button", { name: "Open Agents & styles" }));
    expect(onSelectView).toHaveBeenCalledWith("agents");
    await user.click(screen.getByRole("tab", { name: "Styles" }));
    await user.click(await screen.findByRole("button", { name: "Open Agents & styles" }));
    expect(onSelectView).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole("tab", { name: "Skills" }));
    expect(await screen.findByText(/built by MIG-GUI-3/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open Agents & styles" })).toBeNull();
  });

  it("exports both panels for the Library shell", () => {
    expect(typeof AgentsTab).toBe("function");
    expect(typeof StylesTab).toBe("function");
  });
});
