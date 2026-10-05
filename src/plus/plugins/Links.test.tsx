import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "../PlusViews";
import type { PlusView } from "../nav";
import { golden as hooksGolden } from "../hooks/testkit";
import { createPluginsBridge, wire, type Bridge } from "./testkit";

function Host({ start }: { start: PlusView }) {
  const [view, setView] = useState<PlusView>(start);
  return <PlusViews view={view} onSelectView={(next) => setView(next as PlusView)} />;
}

beforeEach(() => {
  window.localStorage.clear();
  const bridge = createPluginsBridge();
  bridge.set("hooks ls", () => hooksGolden("hooks-ls.full"));
  wire({ invoke, listen }, bridge as Bridge);
});

describe("Plugins and Hooks lead to each other", () => {
  it("goes from a plugin's hooks to Context > Hooks and back to Library > Plugins", async () => {
    const user = userEvent.setup();
    render(<Host start="library" />);
    await user.click(await screen.findByRole("tab", { name: "Plugins" }));
    await screen.findByRole("region", { name: "Plugin ecc" });
    await user.click(screen.getByRole("button", { name: /See them in Context > Hooks/ }));
    expect(
      await screen.findByRole("tab", { name: "Hooks", selected: true }),
    ).toBeVisible();
    await screen.findByRole("group", { name: "Hook counts" });
    await user.click(screen.getByRole("button", { name: "Plugins" }));
    expect(
      await screen.findByRole("tab", { name: "Plugins", selected: true }),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        within(screen.getByRole("group", { name: "Plugins" })).getAllByText("ecc")[0],
      ).toBeVisible(),
    );
  });
});
